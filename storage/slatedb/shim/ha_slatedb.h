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

/* Stage 0 checkpoint 2: ha_slatedb dispatches lifecycle / lock /
   extra methods through the cxx Rust bridge. Each ha_slatedb owns a
   `rust::Box<slatedb::HaSlateDb>` whose drop runs when the handler
   is destroyed. Per the cxx contract, memory ownership stays on the
   side that allocated — the Rust crate built the HaSlateDb instance
   (via `slatedb::new_ha_slatedb()`) and the Box's dtor drops it
   back into Rust. */

#include "my_global.h"
#include "thr_lock.h"
#include "handler.h"
#include "my_base.h"
#include "slatedb_bridge/bridge.h"

class ha_slatedb: public handler
{
  THR_LOCK_DATA lock;

  /* Per-handler Rust state — opaque on this side. Constructed via
     `slatedb::new_ha_slatedb()` in the ctor; dropped via the Box's
     dtor when this instance is destroyed. */
  rust::Box<slatedb::HaSlateDb> m_rust;

public:
  ha_slatedb(handlerton *hton, TABLE_SHARE *table_arg);
  ~ha_slatedb() = default;

  ulonglong table_flags() const override { return HA_BINLOG_STMT_CAPABLE; }
  ulong index_flags(uint, uint, bool) const override { return 0; }

  uint max_supported_record_length() const override { return HA_MAX_REC_LENGTH; }
  uint max_supported_keys()           const override { return 0; }
  uint max_supported_key_parts()      const override { return 0; }
  uint max_supported_key_length()     const override { return 0; }

  IO_AND_CPU_COST scan_time() override
  {
    IO_AND_CPU_COST cost;
    cost.io= (double)(stats.records + stats.deleted) * DISK_READ_COST;
    cost.cpu= 0;
    return cost;
  }
  IO_AND_CPU_COST rnd_pos_time(ha_rows rows) override
  {
    IO_AND_CPU_COST cost;
    cost.io= 0;
    cost.cpu= (double) rows * DISK_READ_COST;
    return cost;
  }

  int open(const char *name, int mode, uint test_if_locked) override;
  int close() override;

  int rnd_init(bool scan) override;
  int rnd_end() override;
  int rnd_next(uchar *buf) override;
  int rnd_pos(uchar *buf, uchar *pos) override;
  void position(const uchar *record) override;

  int info(uint) override;
  int extra(enum ha_extra_function operation) override;
  int external_lock(THD *thd, int lock_type) override;
  int create(const char *name, TABLE *form, HA_CREATE_INFO *create_info) override;
  int write_row(const uchar *buf) override;

  THR_LOCK_DATA **store_lock(THD *thd, THR_LOCK_DATA **to,
                             enum thr_lock_type lock_type) override;
};
