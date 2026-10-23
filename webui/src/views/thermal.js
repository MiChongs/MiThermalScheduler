// 温控：方案选择顺序、方案、场景规则、过热保护、充电加速
import { html } from 'lit';
import { TRIGGERS, icon, when } from '../ui.js';
import { renderProfiles } from './profiles.js';

function ruleText(r) {
  if (r.kind === 'batteryBelow') return `电量低于 ${r.value}%`;
  if (r.kind === 'tempAbove') return `主板温度 ≥ ${r.value}°C`;
  return TRIGGERS[r.kind]?.desc ?? '';
}

/** 决策顺序说明 */
function renderOrder(app) {
  const st = app.status?.daemon.state;
  const steps = [
    ['shield', '过热保护', st?.guard],
    ['apps', '应用配置', st?.reason?.startsWith('前台应用')],
    ['automation', '场景规则', st?.ruleId && !st?.reason?.startsWith('前台应用')],
    ['star', '默认方案', st?.reason?.startsWith('默认方案')],
  ];
  return html`<div class="order card-outlined">
    <div class="order-title">方案选择顺序</div>
    <div class="order-steps">
      ${steps.map(([ic, name, on], i) => html`${i ? icon('chevron_right', 'tiny dim') : ''}<span class="order-step ${on ? 'on' : ''}">${icon(ic)}${name}</span>`)}
    </div>
    <div class="order-sub">充电期间在所选方案的基础上应用充电加速</div>
  </div>`;
}

function renderSceneRules(app) {
  const cfg = app.cfg;
  const st = app.status?.daemon.state;
  return html`
    <div class="section-head"><h2>场景规则</h2><span class="hint-inline">按顺序匹配</span></div>
    <label class="list-row card-outlined switch-row">
      <div class="avatar small">${icon('automation')}</div>
      <div class="grow">
        <div class="row-title">启用场景规则</div>
        <div class="row-sub">在充电、熄屏、低电量或高温时切换方案。前台应用已设置方案时，以应用配置为准</div>
      </div>
      <md-switch .selected=${cfg.automation} @change=${(e) => app.edit((c) => { c.automation = e.target.selected; })}></md-switch>
    </label>
    ${cfg.rules.length
      ? html`<div class="rule-list">
          ${cfg.rules.map((r, i) => {
            const t = TRIGGERS[r.kind] ?? TRIGGERS.charging;
            const p = app.findProfile(r.profile);
            const active = st?.ruleId === r.id;
            return html`<div class="auto-rule card-outlined ${r.enabled ? '' : 'off'} ${active ? 'active' : ''}">
              <div class="list-row clickable" @click=${() => app.editAutoRule(r)}>
                <div class="avatar small">${icon(t.icon)}</div>
                <div class="grow">
                  <div class="row-title">${t.name}${when(active, () => html`<span class="tag accent">生效中</span>`)}</div>
                  <div class="row-sub">${ruleText(r)}</div>
                  <div class="row-sub to">${icon('chevron_right', 'tiny')}切换到「${p?.name ?? '已删除的方案'}」</div>
                </div>
                <md-switch .selected=${r.enabled} @click=${(e) => e.stopPropagation()} @change=${(e) => app.edit((c) => { c.rules[i].enabled = e.target.selected; })}></md-switch>
              </div>
              <div class="rule-actions">
                <md-icon-button ?disabled=${i === 0} @click=${() => app.edit((c) => { c.rules.splice(i - 1, 0, ...c.rules.splice(i, 1)); })}>${icon('arrow_upward')}</md-icon-button>
                <md-icon-button ?disabled=${i === cfg.rules.length - 1} @click=${() => app.edit((c) => { c.rules.splice(i + 1, 0, ...c.rules.splice(i, 1)); })}>${icon('arrow_downward')}</md-icon-button>
                <span class="grow"></span>
                <md-icon-button @click=${() => app.deleteAutoRule(i)}>${icon('delete')}</md-icon-button>
              </div>
            </div>`;
          })}
        </div>`
      : html`<div class="empty card-outlined">
          ${icon('automation', 'big')}
          <div>暂无场景规则</div>
          <p class="hint flat">示例：熄屏时切换到「清凉」方案。按应用切换方案请在应用页中设置。</p>
        </div>`}
    <md-filled-tonal-button class="block-btn" @click=${() => app.editAutoRule(null)}><span slot="icon">${icon('add')}</span>添加场景规则</md-filled-tonal-button>
  `;
}

function renderGuard(app) {
  const g = app.cfg.guard;
  const st = app.status?.daemon.state;
  return html`
    <div class="section-head"><h2>过热保护</h2>${when(st?.guard, () => html`<span class="tag danger">已触发</span>`)}</div>
    <div class="card-outlined guard ${st?.guard ? 'tripped' : ''}">
      <label class="list-row switch-row">
        <div class="avatar small error">${icon('shield')}</div>
        <div class="grow">
          <div class="row-title">启用过热保护</div>
          <div class="row-sub">${st?.guard ? '已触发，当前临时使用官方温控' : '温度过高时临时恢复官方温控，温度回落后自动解除'}</div>
        </div>
        <md-switch .selected=${g.enabled} @change=${(e) => app.edit((c) => { c.guard.enabled = e.target.selected; })}></md-switch>
      </label>
      ${when(g.enabled, () => html`
        <div class="slider-block">
          <div class="slider-label"><span>触发温度</span><b>${g.trip}°C</b></div>
          <md-slider min="44" max="58" step="1" labeled .value=${g.trip} @change=${(e) => app.edit((c) => { c.guard.trip = e.target.value; if (c.guard.release >= c.guard.trip) c.guard.release = c.guard.trip - 3; })}></md-slider>
          <div class="slider-label"><span>恢复温度</span><b>${g.release}°C</b></div>
          <md-slider min="35" max=${g.trip - 1} step="1" labeled .value=${g.release} @change=${(e) => app.edit((c) => { c.guard.release = e.target.value; })}></md-slider>
          <p class="hint flat">温度以温控传感器 VIRTUAL-SENSOR0 为准，该数值接近机身温度。触发温度同时是各方案与 Joyose 限帧的放宽上限：放宽后的触发温度不会超过该值，保证达到保护温度前温控已逐级生效。</p>
        </div>
      `)}
    </div>
  `;
}

function renderCharge(app) {
  const cb = app.cfg.charge ?? { enabled: true, offset: 4, removeSic: false };
  const st = app.status;
  const ensure = (c) => (c.charge ??= { ...cb });
  const active = st?.battery.charging && st?.daemon.state.effective?.endsWith('+charge');
  return html`
    <div class="section-head"><h2>充电加速</h2>${when(active, () => html`<span class="tag accent">充电加速中</span>`)}</div>
    <div class="card-outlined">
      <label class="list-row switch-row">
        <div class="avatar small">${icon('battery_charging_full')}</div>
        <div class="grow">
          <div class="row-title">启用充电加速</div>
          <div class="row-sub">充电期间放宽充电限流规则，亮屏使用时同样保持快速充电，断开充电器后自动恢复</div>
        </div>
        <md-switch .selected=${cb.enabled} @change=${(e) => app.edit((c) => { ensure(c).enabled = e.target.selected; })}></md-switch>
      </label>
      ${when(cb.enabled, () => html`
        <div class="slider-block">
          <div class="slider-label"><span>充电限流门限上调</span><b>+${cb.offset}°C</b></div>
          <md-slider min="1" max="8" step="1" labeled .value=${cb.offset} @change=${(e) => app.edit((c) => { ensure(c).offset = e.target.value; })}></md-slider>
          <p class="hint flat">官方规则自温控传感器 33°C 起限制充电电流，亮屏使用时充电电流通常被限制在 5 A 左右。上调 4°C 后，限流起点为 37°C。</p>
        </div>
        <label class="list-row switch-row">
          <div class="avatar small error">${icon('bolt')}</div>
          <div class="grow">
            <div class="row-title">移除 SIC 电流控制</div>
            <div class="row-sub">移除按温度闭环调节充电电流的 SIC-BAT 规则，充电速度更快，发热相应增加</div>
          </div>
          <md-switch .selected=${cb.removeSic} @change=${async (e) => {
            const on = e.target.selected;
            if (on && !(await app.ask({ title: '移除 SIC 电流控制？', body: '充电时将不再按温度闭环降低电流，电池温度会相应升高。电池自身的温度保护与过热保护仍然有效。', ok: '移除', danger: true }))) {
              e.target.selected = false;
              return;
            }
            app.edit((c) => { ensure(c).removeSic = on; });
          }}></md-switch>
        </label>
      `)}
    </div>
  `;
}

export function renderThermal(app) {
  return html`
    ${renderOrder(app)}
    ${renderProfiles(app)}
    ${renderSceneRules(app)}
    ${renderGuard(app)}
    ${renderCharge(app)}
  `;
}
