package My::Suite::Slatedb;

@ISA = qw(My::Suite);
use strict;

return "SlateDB plugin not compiled" unless $ENV{HA_SLATEDB_SO};

bless { };
