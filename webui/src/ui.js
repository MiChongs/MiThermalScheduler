// 通用视图片段：图标、仪表盘、折线、格式化
import { html, svg, nothing } from 'lit';
import { ICONS } from './icons.gen.js';

export const icon = (name, cls = '') =>
  html`<svg class="ic ${cls}" viewBox="0 -960 960 960" aria-hidden="true"><path d=${ICONS[name] ?? ICONS.help}></path></svg>`;

export const CAT_ICONS = {
  cpu: 'memory',
  gpu: 'deployed_code',
  npu: 'neurology',
  charge: 'battery_charging_full',
  display: 'light_mode',
  net: 'cell_tower',
  warn: 'warning',
  other: 'more_horiz',
  sensor: 'sensors',
  bcl: 'battery_alert',
};

export const TRIGGERS = {
  charging: { name: '充电时', icon: 'battery_charging_full', desc: '连接充电器时' },
  screenOff: { name: '熄屏时', icon: 'mobile_off', desc: '屏幕关闭时' },
  batteryBelow: { name: '低电量', icon: 'battery_alert', desc: '电量低于设定值时' },
  tempAbove: { name: '高温时', icon: 'device_thermostat', desc: '主板温度达到设定值时' },
};

export const PROFILE_ICONS = ['tune', 'verified', 'ac_unit', 'eco', 'balance', 'speed', 'sports_esports', 'bolt', 'local_fire_department', 'rocket_launch', 'nightlight', 'star', 'favorite', 'science'];

export const RISK = [null, { label: '注意', cls: 'warn' }, { label: '高风险', cls: 'danger' }];

export const mhz = (khz) => Math.round(khz / 1000);
/** "4 5 6 7" → "4–7" */
export const cpuRange = (cpus) => {
  const l = cpus.trim().split(/\s+/);
  return l.length > 1 ? `${l[0]}–${l[l.length - 1]}` : l[0];
};
export const ghz = (khz) => (khz / 1e6).toFixed(2);
export const fmtT = (t) => (Number.isFinite(t) ? t.toFixed(1) : '--');

export function tempTone(t) {
  if (t >= 48) return 'hot';
  if (t >= 42) return 'warm';
  return '';
}

/** 270° 圆弧温度仪表 */
export function gauge(value, min = 20, max = 60) {
  const r = 52;
  const c = 2 * Math.PI * r;
  const sweep = 0.75;
  const ratio = Math.min(1, Math.max(0, (value - min) / (max - min)));
  return svg`<svg class="gauge" viewBox="0 0 128 128">
    <circle class="gauge-track" cx="64" cy="64" r=${r} stroke-dasharray="${c * sweep} ${c}" transform="rotate(135 64 64)"></circle>
    <circle class="gauge-value ${tempTone(value)}" cx="64" cy="64" r=${r} stroke-dasharray="${c * sweep * ratio} ${c}" transform="rotate(135 64 64)"></circle>
  </svg>`;
}

/** 多序列折线图 */
export function sparkline(series, { height = 64, width = 300 } = {}) {
  const all = series.flatMap((s) => s.data);
  if (all.length < 2) return html`<div class="spark-empty">正在采集数据…</div>`;
  const lo = Math.floor(Math.min(...all) - 1);
  const hi = Math.ceil(Math.max(...all) + 1);
  const n = Math.max(...series.map((s) => s.data.length));
  const pts = (d) => d.map((v, i) => `${((i + n - d.length) / (n - 1)) * width},${height - ((v - lo) / (hi - lo)) * height}`).join(' ');
  return html`<div class="spark">
    <svg viewBox="0 0 ${width} ${height}" preserveAspectRatio="none">
      ${series.map((s) => svg`<polyline class="line ${s.cls}" points=${pts(s.data)}></polyline>`)}
    </svg>
    <div class="spark-axis"><span>${hi}°</span><span>${lo}°</span></div>
  </div>`;
}

export const when = (cond, tpl) => (cond ? tpl() : nothing);

export const clone = (o) => JSON.parse(JSON.stringify(o));

export function uid(prefix) {
  return `${prefix}${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
}
