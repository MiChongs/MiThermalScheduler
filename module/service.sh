#!/system/bin/sh
MODDIR=${0%/*}

until [ "$(getprop sys.boot_completed)" = "1" ]; do sleep 2; done
sleep 5

setsid nohup $MODDIR/bin/thermalctl daemon > /dev/null 2>&1 < /dev/null &
