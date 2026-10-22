// 游戏调度：Joyose 调教
import { html } from 'lit';
import { cpuRange, icon, mhz, when } from '../ui.js';

const FPS_MODES = [
  ['stock', '官方'],
  ['relax', '放宽'],
  ['off', '关闭'],
];
const CPU_MODES = [
  ['stock', '官方'],
  ['raise', '提高'],
  ['max', '满频'],
];
const CLUSTER = { 0: '小核', 2: '大核', 5: '大核', 7: '超大核' };

/** "10:0,45:110,46:90" → "45°C 时 110 帧，46°C 时 90 帧" */
function fmtTable(s) {
  if (!s) return '—';
  const items = s
    .split(',')
    .map((it) => it.split(':'))
    .filter(([t]) => Number(t) >= 30)
    .map(([t, v]) => `${t}°C 时${Number(v) === 0 ? '不限帧' : ` ${v} 帧`}`);
  return items.length ? items.join('，') : '不限帧';
}

/** "com.tencent.tmgp.sgame@120#10:0,45:110" → 120 档的表 */
function fmtCgame(list) {
  const s = (list ?? []).find((x) => x.startsWith('com.tencent.tmgp.sgame@'));
  if (!s) return '—';
  const seg = s.split('@')[1].split(';').find((x) => x.startsWith('120#'));
  return seg ? fmtTable(seg.split('#')[1]) : '—';
}

/** "120#10:0 0 0 0 0 0,44.5:45 120 90 20 2 3" → "44.5°C 起控温 45°C（90–120 帧）" */
function fmtPid(s) {
  if (!s) return '已移除';
  const item = s.split('#')[1]?.split(',').find((x) => Number(x.split(':')[0]) >= 30);
  if (!item) return '—';
  const [t, params] = item.split(':');
  const [target, max, min] = params.split(' ');
  return `${t}°C 起将温度控制在 ${target}°C（${min}–${max} 帧）`;
}

const fmtCaps = (arr) => (arr?.length ? `${arr.map((v) => (v / 1e6).toFixed(2)).join(' / ')} GHz` : '—');

function compare(st) {
  const o = st.official ?? {};
  const t = st.tuned ?? {};
  const rows = [
    ['王者荣耀温控限帧', fmtCgame(o.cgameDf), fmtCgame(t.cgameDf)],
    ['王者荣耀 PID 温度控制', fmtPid(o.sgamePid120), fmtPid(t.sgamePid120)],
    ['全局限帧（性能优化开启）', fmtTable(o.dynamicFps), fmtTable(t.dynamicFps)],
    ['全局限帧（性能优化关闭）', fmtTable(o.dynamicFpsM), fmtTable(t.dynamicFpsM)],
    ['通用游戏 CPU 上限', fmtCaps(o.foregroundMa), fmtCaps(t.foregroundMa)],
    ['王者荣耀限帧 110 时的 CPU 上限', fmtCaps(o.sgameDfMa?.['110']), fmtCaps(t.sgameDfMa?.['110'])],
  ];
  return html`<div class="cmp">
    ${rows.map(([name, a, b]) => html`<div class="cmp-row">
      <div class="cmp-name">${name}</div>
      <div class="cmp-vals">
        <span class="cmp-old">${a}</span>
        ${a === b ? html`<span class="cmp-same">不变</span>` : html`${icon('chevron_right', 'tiny')}<span class="cmp-new">${b}</span>`}
      </div>
    </div>`)}
  </div>`;
}

export function renderJoyose(app) {
  const jc = app.cfg.joyose ?? { enabled: false, fpsMode: 'relax', fpsOffset: 4, cpuMode: 'raise', cpuFloor: [2265600, 2707200, 2707200, 2745600] };
  const st = app.joyose;
  const ensure = (c) => (c.joyose ??= { ...jc });
  const set = (fn) => app.edit((c) => fn(ensure(c))).then(() => app.loadJoyose(4000));

  let badge;
  if (!st) badge = html`<span class="hint-inline">正在读取…</span>`;
  else if (!st.installed) badge = html`<span class="hint-inline">未安装 Joyose</span>`;
  else if (st.active) badge = html`<span class="tag accent">已生效</span>`;
  else if (st.injected) badge = html`<span class="tag warn">等待加载</span>`;
  else badge = html`<span class="tag">官方配置</span>`;

  return html`
    <div class="section-head"><h2>游戏调度（Joyose）</h2>${badge}</div>
    <div class="card-outlined">
      <label class="list-row switch-row">
        <div class="avatar small">${icon('sports_esports')}</div>
        <div class="grow">
          <div class="row-title">启用 Joyose 调教</div>
          <div class="row-sub">调整系统游戏加速服务的温控限帧与 CPU 上限，官方配置更新后自动重新应用</div>
        </div>
        <md-switch .selected=${jc.enabled} ?disabled=${st && !st.installed} @change=${(e) => set((j) => { j.enabled = e.target.selected; })}></md-switch>
      </label>
      ${when(jc.enabled, () => html`
        <div class="cat-row">
          <div class="cat-head">
            <div class="avatar small">${icon('device_thermostat')}</div>
            <div class="grow">
              <div class="row-title">温控限帧</div>
              <div class="row-sub">游戏运行时按温度降低帧率，包括王者荣耀的 PID 温度控制</div>
            </div>
          </div>
          <div class="seg">
            ${FPS_MODES.map(([m, label]) => html`<button class=${jc.fpsMode === m ? 'on' : ''} @click=${async () => {
              if (m === 'off' && !(await app.ask({ title: '关闭温控限帧？', body: '游戏将不再因温度降低帧率，机身温度会明显升高。请保持过热保护开启。', ok: '关闭', danger: true }))) return;
              set((j) => { j.fpsMode = m; });
            }}>${jc.fpsMode === m ? icon('check') : ''}${label}</button>`)}
          </div>
          ${when(jc.fpsMode === 'relax', () => html`<div class="slider-row">
            <md-slider min="1" max="10" step="1" labeled .value=${jc.fpsOffset} @change=${(e) => set((j) => { j.fpsOffset = e.target.value; })}></md-slider>
            <span class="slider-val">+${jc.fpsOffset}°C</span>
          </div>`)}
        </div>
        <div class="cat-row">
          <div class="cat-head">
            <div class="avatar small">${icon('memory')}</div>
            <div class="grow">
              <div class="row-title">游戏 CPU 上限</div>
              <div class="row-sub">游戏运行及降帧期间的 CPU 频率上限</div>
            </div>
          </div>
          <div class="seg">
            ${CPU_MODES.map(([m, label]) => html`<button class=${jc.cpuMode === m ? 'on' : ''} @click=${() => set((j) => { j.cpuMode = m; })}>${jc.cpuMode === m ? icon('check') : ''}${label}</button>`)}
          </div>
          ${when(jc.cpuMode === 'raise', () => html`<div class="floor-grid">
            ${app.info.clusters.map((c, i) => html`<label class="floor-item">
              <span>${CLUSTER[c.first] ?? c.policy}<small>CPU ${cpuRange(c.cpus)}</small></span>
              <select class="native-select" .value=${String(jc.cpuFloor[i] ?? 0)} @change=${(e) => set((j) => { j.cpuFloor[i] = Number(e.target.value); })}>
                ${[...c.freqs].reverse().filter((f) => f >= 1e6).map((f) => html`<option value=${String(f)} ?selected=${jc.cpuFloor[i] === f}>不低于 ${mhz(f)} MHz</option>`)}
              </select>
            </label>`)}
          </div>`)}
        </div>
      `)}
    </div>
    ${when(st?.installed && jc.enabled, () => html`
      <div class="card-outlined pad joy-compare">
        <div class="cmp-head">
          <span>官方版本 ${st.officialVersion ?? '—'}</span>
          <span>已加载版本 ${st.loadedVersion ?? '—'}</span>
        </div>
        ${compare(st)}
        <p class="hint flat">调教配置基于设备上最新的官方配置生成，官方配置更新后将自动重新生成。</p>
      </div>
      <md-filled-tonal-button class="block-btn" @click=${() => app.joyoseApply()}><span slot="icon">${icon('restart_alt')}</span>立即重新应用</md-filled-tonal-button>
    `)}
  `;
}
