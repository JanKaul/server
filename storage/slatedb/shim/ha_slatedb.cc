/*
  Copyright (c) 2026, MariaDB Corporation.

  This program is free software; you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation; version 2 of the License.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with this program; if not, write to the Free Software
  Foundation, Inc., 51 Franklin St, Fifth Floor, Boston, MA 02110-1335 USA */

/* Stage 0 checkpoint 2: ha_slatedb now dispatches its lifecycle /
   lock / extra methods through the cxx Rust bridge, the handlerton
   transaction callbacks invoke the Rust side at the right
   boundaries, and the plugin bootstraps an in-memory SlateDB engine
   at load time. CREATE TABLE / COMMIT / ROLLBACK round-trip through
   Rust; row I/O still returns end-of-file (DML lands in later
   slices). */

#include <my_global.h>
#include <mysql/plugin.h>
#include "ha_slatedb.h"
#include "sql_class.h"
#include "log.h"
#include "slatedb_bridge/bridge.h"

static handlerton *slatedb_hton;

/* ----------------------------------------------------------------- */
/*  Status code translation                                          */
/* ----------------------------------------------------------------- */

/* Map a Rust-side `slatedb::status::*` int to a MariaDB HA_ERR_*.
   Coarse-grained at Stage 0 — anything non-OK collapses to
   HA_ERR_INTERNAL_ERROR. Once the error channel is wired through
   the cxx bridge (slatedb::Error → ha_err mapping in
   `error::slatedb_error_to_ha_err`), refine here. */
static int slatedb_status_to_ha_err(int32_t status)
{
  if (status == 0)
    return 0;
  /* status::NOT_SUPPORTED == 4 — maps directly. */
  if (status == 4)
    return HA_ERR_WRONG_COMMAND;
  return HA_ERR_INTERNAL_ERROR;
}

/* ----------------------------------------------------------------- */
/*  Handlerton transaction callbacks                                 */
/*                                                                   */
/*  Static dispatchers that translate MariaDB's `THD *` →            */
/*  `thd_id: u64` (via `thd_get_thread_id`) and forward to the       */
/*  Rust `slatedb_handlerton_*` free functions.                      */
/* ----------------------------------------------------------------- */

static int slatedb_commit(THD *thd, bool all)
{
  int32_t rc= slatedb::slatedb_handlerton_commit(thd_get_thread_id(thd), all);
  return slatedb_status_to_ha_err(rc);
}

static int slatedb_rollback(THD *thd, bool all)
{
  int32_t rc= slatedb::slatedb_handlerton_rollback(thd_get_thread_id(thd), all);
  return slatedb_status_to_ha_err(rc);
}

static int slatedb_close_connection(THD *thd)
{
  int32_t rc= slatedb::slatedb_handlerton_close_connection(thd_get_thread_id(thd));
  return slatedb_status_to_ha_err(rc);
}

static int slatedb_savepoint_set(THD *thd, void *)
{
  int32_t rc= slatedb::slatedb_handlerton_savepoint(thd_get_thread_id(thd));
  return slatedb_status_to_ha_err(rc);
}

static int slatedb_savepoint_rollback(THD *thd, void *)
{
  int32_t rc= slatedb::slatedb_handlerton_rollback_to_savepoint(
      thd_get_thread_id(thd));
  return slatedb_status_to_ha_err(rc);
}

static bool slatedb_savepoint_rollback_can_release_mdl(THD *thd)
{
  return slatedb::slatedb_handlerton_rollback_to_savepoint_can_release_mdl(
      thd_get_thread_id(thd));
}

static void slatedb_commit_ordered(THD *thd, bool all)
{
  slatedb::slatedb_handlerton_commit_ordered(thd_get_thread_id(thd), all);
}

static int slatedb_start_consistent_snapshot(THD *thd)
{
  int32_t rc= slatedb::slatedb_handlerton_start_consistent_snapshot(
      thd_get_thread_id(thd));
  return slatedb_status_to_ha_err(rc);
}

/* ----------------------------------------------------------------- */
/*  Plugin lifecycle                                                 */
/* ----------------------------------------------------------------- */

static handler *slatedb_create_handler(handlerton *hton, TABLE_SHARE *table,
                                       MEM_ROOT *mem_root)
{
  return new (mem_root) ha_slatedb(hton, table);
}

static int slatedb_init_func(void *p)
{
  DBUG_ENTER("slatedb_init_func");
  slatedb_hton= (handlerton *) p;

  /* Required handlerton fields. */
  slatedb_hton->create= slatedb_create_handler;
  slatedb_hton->flags=  HTON_CAN_RECREATE;
  slatedb_hton->drop_table= [](handlerton *, const char *path) -> int {
    int32_t rc= slatedb::slatedb_drop_table(rust::String(path));
    return slatedb_status_to_ha_err(rc);
  };

  /* Transaction callbacks — each dispatches into Rust via the
     cxx bridge. The Rust side owns the per-THD `TxnRegistry`. */
  slatedb_hton->commit= slatedb_commit;
  slatedb_hton->rollback= slatedb_rollback;
  slatedb_hton->close_connection= slatedb_close_connection;
  slatedb_hton->savepoint_set= slatedb_savepoint_set;
  slatedb_hton->savepoint_rollback= slatedb_savepoint_rollback;
  slatedb_hton->savepoint_rollback_can_release_mdl=
      slatedb_savepoint_rollback_can_release_mdl;
  slatedb_hton->commit_ordered= slatedb_commit_ordered;
  slatedb_hton->start_consistent_snapshot= slatedb_start_consistent_snapshot;
  /* savepoint_offset must be set for MariaDB to enable savepoints
     even on engines that return HA_ERR_WRONG_COMMAND for them —
     leaving it at 0 (default) effectively disables the hooks, which
     matches Q10 (Stage 0 has no savepoint support). */

  /* Bootstrap the in-memory SlateDB engine. Failure here aborts
     plugin load — the Rust side returns non-zero from
     `init_in_memory` if the runtime can't start or the engine
     is already installed. */
  int32_t init_rc=
      slatedb::slatedb_init_in_memory(rust::String("slatedb_plugin"));
  if (init_rc != 0)
  {
    sql_print_error("SLATEDB: engine init failed (status=%d)", (int) init_rc);
    DBUG_RETURN(1);
  }

  rust::String version= slatedb::slatedb_version();
  sql_print_information("SLATEDB: cxx bridge live (%.*s)",
                        static_cast<int>(version.size()), version.data());
  DBUG_RETURN(0);
}

static int slatedb_done_func(void *)
{
  DBUG_ENTER("slatedb_done_func");
  int32_t rc= slatedb::slatedb_shutdown();
  if (rc != 0)
    sql_print_warning("SLATEDB: engine shutdown returned status=%d", (int) rc);
  DBUG_RETURN(0);
}

/* ----------------------------------------------------------------- */
/*  ha_slatedb method bodies                                         */
/* ----------------------------------------------------------------- */

ha_slatedb::ha_slatedb(handlerton *hton, TABLE_SHARE *table_arg)
  : handler(hton, table_arg),
    m_rust(slatedb::new_ha_slatedb())
{}

int ha_slatedb::open(const char *name, int, uint)
{
  DBUG_ENTER("ha_slatedb::open");
  thr_lock_data_init(nullptr, &lock, nullptr);
  int32_t rc= m_rust->ha_open(rust::String(name));
  DBUG_RETURN(slatedb_status_to_ha_err(rc));
}

int ha_slatedb::close()
{
  DBUG_ENTER("ha_slatedb::close");
  int32_t rc= m_rust->ha_close();
  DBUG_RETURN(slatedb_status_to_ha_err(rc));
}

int ha_slatedb::rnd_init(bool)
{
  DBUG_ENTER("ha_slatedb::rnd_init");
  DBUG_RETURN(0);
}

int ha_slatedb::rnd_end()
{
  DBUG_ENTER("ha_slatedb::rnd_end");
  DBUG_RETURN(0);
}

int ha_slatedb::rnd_next(uchar *)
{
  DBUG_ENTER("ha_slatedb::rnd_next");
  /* DML / read path lands in a later slice. */
  DBUG_RETURN(HA_ERR_END_OF_FILE);
}

int ha_slatedb::rnd_pos(uchar *, uchar *)
{
  DBUG_ENTER("ha_slatedb::rnd_pos");
  DBUG_RETURN(HA_ERR_WRONG_COMMAND);
}

void ha_slatedb::position(const uchar *)
{
  DBUG_ENTER("ha_slatedb::position");
  DBUG_VOID_RETURN;
}

int ha_slatedb::info(uint)
{
  DBUG_ENTER("ha_slatedb::info");
  DBUG_RETURN(0);
}

int ha_slatedb::extra(enum ha_extra_function operation)
{
  DBUG_ENTER("ha_slatedb::extra");
  int32_t rc= m_rust->ha_extra(static_cast<int32_t>(operation));
  DBUG_RETURN(slatedb_status_to_ha_err(rc));
}

int ha_slatedb::external_lock(THD *thd, int lock_type)
{
  DBUG_ENTER("ha_slatedb::external_lock");

  /* `autocommit_boundary` — at F_UNLCK, should we commit the txn?
     YES iff the connection is NOT inside a multi-statement txn
     (autocommit is on AND no BEGIN active). The C++ method does
     this check because the Rust side can't introspect THD. */
  const bool autocommit_boundary=
      (lock_type == F_UNLCK) && !thd->in_multi_stmt_transaction_mode();

  int32_t rc= m_rust->ha_external_lock(thd_get_thread_id(thd),
                                       lock_type,
                                       autocommit_boundary);
  DBUG_RETURN(slatedb_status_to_ha_err(rc));
}

int ha_slatedb::create(const char *, TABLE *, HA_CREATE_INFO *)
{
  DBUG_ENTER("ha_slatedb::create");
  /* CREATE TABLE no-op for Stage 0 — the catalogue is populated by
     a future slice once the C++ schema description can be threaded
     into TblDef::new + DdlManager::put_and_write. */
  DBUG_RETURN(0);
}

THR_LOCK_DATA **ha_slatedb::store_lock(THD *thd, THR_LOCK_DATA **to,
                                       enum thr_lock_type lock_type)
{
  /* The Rust side decides whether to downgrade the lock type
     (e.g. WRITE_ALLOW_WRITE → WRITE_CONCURRENT_INSERT outside
     LOCK TABLES). It also updates the handler's internal
     `lock_rows` + `db_lock_type`. */
  const bool in_lt= thd_in_lock_tables(thd);
  const bool tspace_op= thd_tablespace_op(thd);
  int32_t chosen= m_rust->ha_store_lock(in_lt, tspace_op,
                                         static_cast<int32_t>(lock_type));
  if (chosen != TL_IGNORE && lock.type == TL_UNLOCK)
    lock.type= static_cast<enum thr_lock_type>(chosen);
  *to++= &lock;
  return to;
}

/* ----------------------------------------------------------------- */
/*  Plugin declaration                                               */
/* ----------------------------------------------------------------- */

struct st_mysql_storage_engine slatedb_storage_engine=
{ MYSQL_HANDLERTON_INTERFACE_VERSION };

maria_declare_plugin(slatedb)
{
  MYSQL_STORAGE_ENGINE_PLUGIN,
  &slatedb_storage_engine,
  "SLATEDB",
  "Embucket",
  "SlateDB storage engine (Stage 0 — cxx bridge live, DML pending)",
  PLUGIN_LICENSE_GPL,
  slatedb_init_func,
  slatedb_done_func,
  0x0001,
  NULL,
  NULL,
  "0.1",
  MariaDB_PLUGIN_MATURITY_EXPERIMENTAL
}
maria_declare_plugin_end;
