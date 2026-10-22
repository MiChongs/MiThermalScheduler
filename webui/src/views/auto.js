import { html } from 'lit';
import { TRIGGERS, icon, when } from '../ui.js';

function ruleText(app, r) {
  const t = TRIGGERS[r.kind];
  if (r.kind === 'app') {
    const names = r.packages.map((p) => app.appLabel(p));
    return names.length ? `${names.slice(0, 3).join('、')}${names.length > 3 ? ` 等 ${names.length} 个应用` : ''}` : '未选择应用';
  }
  if (r.kind === 'batteryBelow') return `电量低于 ${r.value}%`;
  if (r.kind === 'tempAbove') return `主板温度 ≥ ${r.value}°C`;
  return t.desc;
}

export function renderAuto(app) {
  const cfg = app.cfg;
  const g = cfg.guard;
  const st = app.status?.daemon.state;
  return html`
    <div class="section-head first"><h2>过热保护</h2></div>
    <div class="card-outlined guard ${st?.guard ? 'tripped' : ''}">
      <label class="list-row switch-row">
        <div class="avatar small error">${icon('shield')}</div>
        <div class="grow">
          <div class="row-title">启用过热保护</div>
          <div class="row-sub">${st?.guard ? '已触发：当前临时使用官方温控' : '温度过高时临时切回官方温控，降温后自动恢复'}</div>
        </div>
        <md-switch .selected=${g.enabled} @change=${(e) => app.edit((c) => { c.guard.enabled = e.target.selected; })}></md-switch>
      </label>
      ${when(g.enabled, () => html`
        <div class="slider-block">
          <div class="slider-label"><span>触发温度</span><b>${g.trip}°C</b></div>
          <md-slider min="40" max="60" step="1" labeled .value=${g.trip} @change=${(e) => app.edit((c) => { c.guard.trip = e.target.value; if (c.guard.release >= c.guard.trip) c.guard.release = c.guard.trip - 3; })}></md-slider>
          <div class="slider-label"><span>恢复温度</span><b>${g.release}°C</b></div>
          <md-slider min="35" max=${g.trip - 1} step="1" labeled .value=${g.release} @change=${(e) => app.edit((c) => { c.guard.release = e.target.value; })}></md-slider>
          <p class="hint flat">以温控传感器（VIRTUAL-SENSOR0，近似机身温度）为准。官方高温提示约从 45°C 开始。</p>
        </div>
      `)}
    </div>

    <div class="section-head"><h2>自动切换</h2></div>
    <label class="list-row card-outlined switch-row">
      <div class="avatar small">${icon('automation')}</div>
      <div class="grow">
        <div class="row-title">启用自动切换</div>
        <div class="row-sub">按顺序匹配，第一条满足条件的规则生效；都不满足时使用默认方案</div>
      </div>
      <md-switch .selected=${cfg.automation} @change=${(e) => app.edit((c) => { c.automation = e.target.selected; })}></md-switch>
    </label>

    ${cfg.rules.length
      ? html`<div class="rule-list">
          ${cfg.rules.map((r, i) => {
            const t = TRIGGERS[r.kind];
            const p = app.findProfile(r.profile);
            const active = st?.ruleId === r.id;
            return html`<div class="auto-rule card-outlined ${r.enabled ? '' : 'off'} ${active ? 'active' : ''}">
              <div class="list-row clickable" @click=${() => app.editAutoRule(r)}>
                <div class="avatar small">${icon(t.icon)}</div>
                <div class="grow">
                  <div class="row-title">${t.name}${when(active, () => html`<span class="tag accent">生效中</span>`)}</div>
                  <div class="row-sub">${ruleText(app, r)}</div>
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
          <div>还没有自动化规则</div>
          <p class="hint flat">例如：打开游戏时切换到「游戏」，充电时切换到「快充」，熄屏时切换到「清凉」。</p>
        </div>`}
    <md-filled-tonal-button class="block-btn" @click=${() => app.editAutoRule(null)}><span slot="icon">${icon('add')}</span>添加规则</md-filled-tonal-button>

    ${renderRefresh(app)}
  `;
}

function renderRefresh(app) {
  const rc = app.cfg.refresh ?? { enabled: true, respectGuard: true, rules: [] };
  const st = app.status?.daemon.state;
  const names = (pkgs) => {
    const n = pkgs.map((p) => app.appLabel(p));
    return `${n.slice(0, 3).join('、')}${n.length > 3 ? ` 等 ${n.length} 个应用` : ''}`;
  };
  const ensure = (c) => (c.refresh ??= { enabled: true, respectGuard: true, rules: [] });
  return html`
    <div class="section-head"><h2>应用刷新率</h2>${when(st?.refreshRate, () => html`<span class="hint-inline">正在锁定 ${st.refreshRate} Hz</span>`)}</div>
    <div class="card-outlined">
      <label class="list-row switch-row">
        <div class="avatar small">${icon('speed')}</div>
        <div class="grow">
          <div class="row-title">启用应用刷新率</div>
          <div class="row-sub">指定应用在前台时锁定刷新率上限，避免「电量与性能」将游戏限制为 60 Hz</div>
        </div>
        <md-switch .selected=${rc.enabled} @change=${(e) => app.edit((c) => { ensure(c).enabled = e.target.selected; })}></md-switch>
      </label>
      <label class="list-row switch-row">
        <div class="avatar small">${icon('shield')}</div>
        <div class="grow">
          <div class="row-title">过热保护时暂停锁定</div>
          <div class="row-sub">过热保护触发期间交还系统控制，降低发热</div>
        </div>
        <md-switch .selected=${rc.respectGuard} @change=${(e) => app.edit((c) => { ensure(c).respectGuard = e.target.selected; })}></md-switch>
      </label>
    </div>
    ${rc.rules.length
      ? html`<div class="rule-list">
          ${rc.rules.map((r, i) => {
            const active = st?.refreshRate && r.packages.includes(st.refreshPkg);
            return html`<div class="auto-rule card-outlined ${r.enabled ? '' : 'off'} ${active ? 'active' : ''}">
              <div class="list-row clickable" @click=${() => app.editRefreshRule(r)}>
                <div class="avatar small">${icon('speed')}</div>
                <div class="grow">
                  <div class="row-title">${r.rate} Hz${when(active, () => html`<span class="tag accent">生效中</span>`)}</div>
                  <div class="row-sub">${names(r.packages)}</div>
                </div>
                <md-switch .selected=${r.enabled} @click=${(e) => e.stopPropagation()} @change=${(e) => app.edit((c) => { c.refresh.rules[i].enabled = e.target.selected; })}></md-switch>
              </div>
              <div class="rule-actions">
                <span class="grow"></span>
                <md-icon-button @click=${() => app.deleteRefreshRule(i)}>${icon('delete')}</md-icon-button>
              </div>
            </div>`;
          })}
        </div>`
      : html`<div class="empty card-outlined">
          ${icon('speed', 'big')}
          <div>还没有刷新率规则</div>
          <p class="hint flat">例如：王者荣耀在前台时锁定 120 Hz，并在游戏内选择「极高」帧率。</p>
        </div>`}
    <md-filled-tonal-button class="block-btn" @click=${() => app.editRefreshRule(null)}><span slot="icon">${icon('add')}</span>添加刷新率规则</md-filled-tonal-button>
  `;
}
