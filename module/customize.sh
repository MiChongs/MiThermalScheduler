SKIPUNZIP=0
DATA=/data/adb/mi_thermal_m3

ui_print " "
ui_print "  小米温控调度  $(grep_prop version $MODPATH/module.prop)"
ui_print "  ──────────────────────────────"

if [ "$ARCH" != "arm64" ]; then
    abort "  ✖ 仅支持 arm64 设备"
fi
if [ ! -f /vendor/bin/mi_thermald ]; then
    abort "  ✖ 未检测到 mi_thermald，本模块仅适用于小米 / Redmi 设备"
fi
if [ ! -f /odm/etc/thermal-normal.conf ] && [ ! -f /vendor/etc/thermal-normal.conf ]; then
    abort "  ✖ 未找到场景温控配置，当前系统不受支持"
fi

set_perm_recursive $MODPATH 0 0 0755 0644
set_perm $MODPATH/bin/thermalctl 0 0 0755

if ! $MODPATH/bin/thermalctl scenes > /dev/null 2>&1; then
    abort "  ✖ 无法解析本机温控配置，当前系统不受支持"
fi

ui_print "  设备  $(getprop ro.product.marketname) ($(getprop ro.product.device))"
ui_print "  平台  $(getprop ro.soc.model)"
ui_print "  系统  $(getprop ro.mi.os.version.name) · Android $(getprop ro.build.version.release)"
ui_print "  ──────────────────────────────"

mkdir -p $DATA
if [ -f $DATA/config.json ]; then
    ui_print "  ✔ 已保留现有方案与自动化规则"
else
    ui_print "  ✔ 默认不改动温控，启用前保持系统官方策略"
fi

# 模块被停用或删除后，由此脚本在开机时清理遗留的覆盖文件
mkdir -p /data/adb/service.d
cp -f $MODPATH/cleanup.sh /data/adb/service.d/mi_thermal_m3_cleanup.sh
chmod 0755 /data/adb/service.d/mi_thermal_m3_cleanup.sh

ui_print "  ✔ 安装完成，重启后在 KernelSU 中打开 WebUI 进行配置"
ui_print " "
