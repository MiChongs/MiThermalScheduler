import { html } from 'lit';
import { fmtT, gauge, icon, mhz, sparkline, tempTone, when } from '../ui.js';

const CLUSTER = { 0: '小核', 2: '大核', 5: '大核', 7: '超大核' };

function statusBanner(app, s) {
  const st = s.daemon.state;
  if (s.moduleDisabled) return { tone: 'muted', icon: 'power_settings_new', title: '模块已停用', sub: '系统官方温控已恢复' };
  if (!s.daemon.running) return { tone: 'error', icon: 'warning', title: '守护进程未运行', sub: '重启手机后自动启动；也可在「设置」中手动启动' };
  if (st.error) return { tone: 'error', icon: 'warning', title: '应用失败，已回退官方温控', sub: st.error };
  if (st.guard) return { tone: 'error', icon: 'shield', title: '过热保护已触发', sub: st.reason };
  if (!s.enabled || st.effective === 'stock') return { tone: 'muted', icon: 'verified', title: '官方温控', sub: s.enabled ? st.reason : '总开关已关闭' };
  return { tone: 'ok', icon: app.profileIcon(st.effective), title: st.effectiveName, sub: `${st.reason} · 接管 ${st.installed} 个场景` };
}

export function renderHome(app) {
  const s = app.status;
  if (!s) return html`<div class="placeholder">正在读取状态…</div>`;
  const b = statusBanner(app, s);
  const profiles = app.allProfiles();
  return html`
    <section class="hero card-filled">
      <div class="hero-gauge">
        ${gauge(s.board)}
        <div class="hero-center">
          <div class="hero-temp ${tempTone(s.board)}">${fmtT(s.board)}<small>°C</small></div>
          <div class="hero-cap">温控传感器</div>
        </div>
      </div>
      <div class="hero-side">
        <div class="status-pill ${b.tone}">${icon(b.icon)}<span>${b.title}</span></div>
        <div class="hero-sub">${b.sub}</div>
        <div class="hero-meta">
          <div><span>场景</span><b>${s.sceneLabel}</b></div>
          <div><span>sconfig</span><b>${s.sconfig}</b></div>
          ${when(s.daemon.state.foreground, () => html`<div><span>前台</span><b class="ellipsis">${app.appLabel(s.daemon.state.foreground)}</b></div>`)}
          ${when(s.daemon.state.refreshRate, () => html`<div><span>刷新率</span><b>${s.daemon.state.refreshRate} Hz 锁定</b></div>`)}
        </div>
      </div>
    </section>

    ${features(app, s)}

    <div class="section-head"><h2>快速切换</h2><span class="hint-inline">设为默认方案</span></div>
    <div class="chip-scroll">
      ${profiles.map(
        (p) => html`<button class="pchip ${app.cfg.enabled && app.cfg.active === p.id ? 'on' : ''} ${p.id === 'stock' && !app.cfg.enabled ? 'on' : ''}" @click=${() => app.quickSwitch(p.id)}>
          ${icon(p.icon || 'tune')}<span>${p.name}</span>
        </button>`,
      )}
    </div>

    <div class="section-head"><h2>温度趋势</h2><span class="legend"><i class="l1"></i>主板 <i class="l2"></i>CPU</span></div>
    <div class="card-outlined pad">
      ${sparkline([
        { data: app.hist.cpu, cls: 'l2' },
        { data: app.hist.board, cls: 'l1' },
      ])}
    </div>

    <div class="section-head"><h2>温度</h2></div>
    <div class="tiles">
      ${s.temps.map(
        (t) => html`<div class="tile ${tempTone(t.value)}">
          <div class="tile-label">${t.label}</div>
          <div class="tile-val">${fmtT(t.value)}<small>°C</small></div>
        </div>`,
      )}
    </div>

    <div class="section-head"><h2>性能上限</h2><span class="hint-inline">当前 / 温控上限 / 硬件最高</span></div>
    <div class="card-outlined pad freq">
      ${s.clusters.map((c) => freqRow(`${CLUSTER[c.first] ?? c.policy}`, `CPU ${c.cpus.replace(/ /g, '·')}`, mhz(c.cur), mhz(c.max), mhz(c.hw)))}
      ${freqRow('GPU', `等级 ${s.gpu.level}`, Math.round(s.gpu.cur / 1e6), Math.round(s.gpu.max / 1e6), Math.round(s.gpu.hw / 1e6))}
    </div>

    <div class="section-head"><h2>电池</h2></div>
    <div class="card-outlined pad kv4">
      <div><span>电量</span><b>${s.battery.level}%</b></div>
      <div><span>${s.battery.charging ? (s.daemon.state.effective?.endsWith('+charge') ? '充电 · 加速' : '充电') : '放电'}</span><b>${Math.abs(s.battery.currentMa)} mA</b></div>
      <div><span>温度</span><b>${fmtT(s.battery.temp)}°C</b></div>
      <div><span>屏幕</span><b>${s.screenOn ? '亮屏' : '熄屏'}</b></div>
    </div>
  `;
}

function freqRow(name, sub, cur, max, hw) {
  const limited = max < hw;
  return html`<div class="freq-row">
    <div class="freq-top">
      <span class="freq-name">${name}<small>${sub}</small></span>
      <span class="freq-num"><b>${cur}</b> / <b class=${limited ? 'limited' : ''}>${max}</b> / ${hw} MHz</span>
    </div>
    <div class="bar">
      <div class="bar-cap" style="width:${hw ? (max / hw) * 100 : 0}%"></div>
      <div class="bar-cur" style="left:${hw ? Math.min(100, (cur / hw) * 100) : 0}%"></div>
    </div>
  </div>`;
}

/** 各功能当前状态 */
function features(app, s) {
  const st = s.daemon.state;
  const cfg = app.cfg;
  const items = [
    ['shield', '过热保护', !cfg.guard.enabled ? '关闭' : st.guard ? '已触发' : `${cfg.guard.trip}°C`, st.guard ? 'danger' : cfg.guard.enabled ? 'on' : '', 'thermal'],
    ['battery_charging_full', '充电加速', !cfg.charge?.enabled ? '关闭' : st.effective?.endsWith('+charge') ? '生效中' : '待充电', st.effective?.endsWith('+charge') ? 'on' : '', 'thermal'],
    ['speed', '刷新率', st.refreshRate ? `${st.refreshRate} Hz` : cfg.refresh?.enabled === false ? '关闭' : '待机', st.refreshRate ? 'on' : '', 'apps'],
    ['sports_esports', 'Joyose', !cfg.joyose?.enabled ? '官方' : st.joyose ? '已调教' : '未生效', st.joyose ? 'on' : '', 'apps'],
  ];
  return html`<div class="features">
    ${items.map(([ic, name, val, tone, page]) => html`<button class="feature ${tone}" @click=${() => app.setPage(page)}>
      ${icon(ic)}<span class="feature-name">${name}</span><span class="feature-val">${val}</span>
    </button>`)}
  </div>`;
}
