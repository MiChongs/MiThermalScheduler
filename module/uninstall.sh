#!/system/bin/sh
MODDIR=${0%/*}
P=$(cat /data/adb/mi_thermal_m3/daemon.pid 2>/dev/null)
[ -n "$P" ] && kill "$P" 2>/dev/null
sh /data/adb/service.d/mi_thermal_m3_cleanup.sh 2>/dev/null
rm -rf /data/adb/mi_thermal_m3
rm -f /data/adb/service.d/mi_thermal_m3_cleanup.sh
