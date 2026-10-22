import { html } from 'lit';
import { CAT_ICONS, icon } from '../ui.js';

const devCat = (d) =>
  /^(cpu|hotplug|lmh)/.test(d) ? 'cpu' : /^gpu/.test(d) ? 'gpu' : /^cdsp/.test(d) ? 'npu' : /^(battery|thermal_fcc|wireless|wls)/.test(d) ? 'charge' : /bright|backlight/.test(d) ? 'display' : /^(modem|wifi|market|ntn|poor)/.test(d) ? 'net' : /^temp_state/.test(d) ? 'warn' : 'other';

export function renderLogs(app) {
  const lg = app.logs;
  const tab = app.logTab;
  const filters = [
    ['all', '全部'],
    ['cpu', 'CPU'],
    ['gpu', 'GPU'],
    ['charge', '充电'],
  ];
  return html`
    <div class="seg wide">
      <button class=${tab === 'thermal' ? 'on' : ''} @click=${() => app.setLogTab('thermal')}>温控动作</button>
      <button class=${tab === 'module' ? 'on' : ''} @click=${() => app.setLogTab('module')}>模块日志</button>
    </div>
    ${!lg
      ? html`<div class="placeholder">正在读取…</div>`
      : tab === 'thermal'
        ? html`
            <div class="chip-scroll">
              ${filters.map(([id, label]) => html`<button class="pchip ${app.logFilter === id ? 'on' : ''}" @click=${() => { app.logFilter = id; app.requestUpdate(); }}>${label}</button>`)}
            </div>
            <div class="card-outlined log-list">
              ${lg.thermal
                .filter((x) => app.logFilter === 'all' || devCat(x.device) === app.logFilter)
                .slice(0, 120)
                .map(
                  (x) => html`<div class="log-row">
                    <div class="avatar small">${icon(CAT_ICONS[devCat(x.device)])}</div>
                    <div class="grow">
                      <div class="row-title mono">${x.rule}</div>
                      <div class="row-sub">${x.device} → <b>${x.value}</b></div>
                    </div>
                    <div class="log-meta"><b>${x.reading}</b><span>${x.time.split(' ')[1]}</span></div>
                  </div>`,
                )}
            </div>`
        : html`<div class="card-outlined pad mono-block">${lg.module.length ? lg.module.join('\n') : '暂无日志'}</div>`}
  `;
}
