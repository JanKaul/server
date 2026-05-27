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

/* Stage 0 checkpoint 1: bare-minimum ha_slatedb skeleton — plugin loads
   and SHOW ENGINES lists SLATEDB. CREATE TABLE ... ENGINE=SLATEDB will
   succeed but the table is a no-op; all DML returns end-of-file or
   unsupported. Subsequent Stage 0 checkpoints wire in cxx and SlateDB. */

#include <my_global.h>
#include <mysql/plugin.h>
#include "ha_slatedb.h"
#include "sql_class.h"

static handlerton *slatedb_hton;

static handler *slatedb_create_handler(handlerton *hton, TABLE_SHARE *table,
                                       MEM_ROOT *mem_root)
{
  return new (mem_root) ha_slatedb(hton, table);
}

static int slatedb_init_func(void *p)
{
  DBUG_ENTER("slatedb_init_func");
  slatedb_hton= (handlerton *) p;
  slatedb_hton->create= slatedb_create_handler;
  slatedb_hton->flags=  HTON_CAN_RECREATE;
  slatedb_hton->drop_table= [](handlerton *, const char *) { return -1; };
  DBUG_RETURN(0);
}

ha_slatedb::ha_slatedb(handlerton *hton, TABLE_SHARE *table_arg)
  : handler(hton, table_arg)
{}

int ha_slatedb::open(const char *, int, uint)
{
  DBUG_ENTER("ha_slatedb::open");
  thr_lock_data_init(nullptr, &lock, nullptr);
  DBUG_RETURN(0);
}

int ha_slatedb::close()
{
  DBUG_ENTER("ha_slatedb::close");
  DBUG_RETURN(0);
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

int ha_slatedb::external_lock(THD *, int)
{
  DBUG_ENTER("ha_slatedb::external_lock");
  DBUG_RETURN(0);
}

int ha_slatedb::create(const char *, TABLE *, HA_CREATE_INFO *)
{
  DBUG_ENTER("ha_slatedb::create");
  DBUG_RETURN(0);
}

THR_LOCK_DATA **ha_slatedb::store_lock(THD *, THR_LOCK_DATA **to,
                                       enum thr_lock_type lock_type)
{
  if (lock_type != TL_IGNORE && lock.type == TL_UNLOCK)
    lock.type= lock_type;
  *to++= &lock;
  return to;
}

struct st_mysql_storage_engine slatedb_storage_engine=
{ MYSQL_HANDLERTON_INTERFACE_VERSION };

maria_declare_plugin(slatedb)
{
  MYSQL_STORAGE_ENGINE_PLUGIN,
  &slatedb_storage_engine,
  "SLATEDB",
  "Embucket",
  "SlateDB storage engine (Stage 0 skeleton — no storage yet)",
  PLUGIN_LICENSE_GPL,
  slatedb_init_func,
  NULL,
  0x0001,
  NULL,
  NULL,
  "0.1",
  MariaDB_PLUGIN_MATURITY_EXPERIMENTAL
}
maria_declare_plugin_end;
