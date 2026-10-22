#!/system/bin/sh
# 小米温控调度：模块被停用或删除时清理遗留的温控覆盖文件
MOD=/data/adb/modules/mi_thermal_m3
DATA=/data/adb/mi_thermal_m3
CFG=/data/vendor/thermal/config

[ -d $MOD ] && [ ! -f $MOD/disable ] && exit 0

if [ -f $DATA/installed.list ]; then
    while read -r f; do
        [ -n "$f" ] || continue
        chattr -i "$CFG/$f" 2>/dev/null
        rm -f "$CFG/$f"
        if [ -f "$DATA/backup/$f" ]; then
            mv -f "$DATA/backup/$f" "$CFG/$f"
            chown root:system "$CFG/$f"
            chmod 0644 "$CFG/$f"
        fi
    done < $DATA/installed.list
    : > $DATA/installed.list
fi

[ -d $MOD ] || { rm -rf $DATA; rm -f "$0"; }
