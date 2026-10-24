// 应用：应用配置、刷新率锁定、游戏调度
import { html } from 'lit';
import { icon, when } from '../ui.js';
import { renderJoyose } from './joyose.js';

function names(app, pkgs) {
  const n = pkgs.map((p) => app.appLabel(p));
  return n.length ? `${n.slice(0, 3).join('、')}${n.length > 3 ? ` 等 ${n.length} 个` : ''}` : '未选择应用';
}

function renderAppList(app) {
  const apps = app.cfg.apps ?? [];
  const st = app.status?.daemon.state;
  return html`
    <div class="section-head first"><h2>应用配置</h2><span class="hint-inline">优先于场景规则</span></div>
    ${apps.length
      ? html`<div class="rule-list">
          ${apps.map((a, i) => {
            const p = a.profile ? app.findProfile(a.profile) : null;
            const activeProfile = st?.ruleId === a.id;
            const activeRate = st?.refreshRate && a.packages.includes(st.refreshPkg);
            return html`<div class="auto-rule card-outlined ${a.enabled ? '' : 'off'} ${activeProfile || activeRate ? 'active' : ''}">
              <div class="list-row clickable" @click=${() => app.editApp(a)}>
                <div class="letter">${app.appLabel(a.packages[0] ?? '?').slice(0, 1)}</div>
                <div class="grow">
                  <div class="row-title">${names(app, a.packages)}${when(activeProfile || activeRate, () => html`<span class="tag accent">前台</span>`)}</div>
                  <div class="app-chips">
                    <span class="mini-chip ${a.profile ? '' : 'muted'}">${icon(p?.icon || 'tune', 'tiny')}${a.profile ? (p?.name ?? '已删除的方案') : '不切换方案'}</span>
                    <span class="mini-chip ${a.refresh ? '' : 'muted'}">${icon('speed', 'tiny')}${a.refresh ? `${a.refresh} Hz` : '不锁定刷新率'}</span>
                  </div>
                </div>
                <md-switch .selected=${a.enabled} @click=${(e) => e.stopPropagation()} @change=${(e) => app.edit((c) => { c.apps[i].enabled = e.target.selected; })}></md-switch>
              </div>
              <div class="rule-actions">
                <span class="grow"></span>
                <md-icon-button @click=${() => app.deleteApp(i)}>${icon('delete')}</md-icon-button>
              </div>
            </div>`;
          })}
        </div>`
      : html`<div class="empty card-outlined">
          ${icon('apps', 'big')}
          <div>暂无应用配置</div>
          <p class="hint flat">示例：王者荣耀位于前台时使用「游戏」方案，并将刷新率锁定为 120 Hz。</p>
        </div>`}
    <md-filled-tonal-button class="block-btn" @click=${() => app.editApp(null)}><span slot="icon">${icon('add')}</span>添加应用配置</md-filled-tonal-button>
  `;
}

function renderRefreshOptions(app) {
  const rc = app.cfg.refresh ?? { enabled: true, respectGuard: false };
  const ensure = (c) => (c.refresh ??= { enabled: true, respectGuard: false });
  const st = app.status?.daemon.state;
  return html`
    <div class="section-head"><h2>刷新率锁定</h2>${when(st?.refreshRate, () => html`<span class="tag accent">正在锁定 ${st.refreshRate} Hz</span>`)}</div>
    <div class="card-outlined">
      <label class="list-row switch-row">
        <div class="avatar small">${icon('speed')}</div>
        <div class="grow">
          <div class="row-title">启用刷新率锁定</div>
          <div class="row-sub">应用位于前台时按应用配置锁定刷新率上限，防止系统将其限制为 60 Hz</div>
        </div>
        <md-switch .selected=${rc.enabled} @change=${(e) => app.edit((c) => { ensure(c).enabled = e.target.selected; })}></md-switch>
      </label>
      <label class="list-row switch-row">
        <div class="avatar small">${icon('shield')}</div>
        <div class="grow">
          <div class="row-title">过热保护时暂停锁定</div>
          <div class="row-sub">过热保护触发期间由系统管理刷新率，游戏可能被限制为 60 Hz</div>
        </div>
        <md-switch .selected=${rc.respectGuard} @change=${(e) => app.edit((c) => { ensure(c).respectGuard = e.target.selected; })}></md-switch>
      </label>
    </div>
  `;
}

export function renderApps(app) {
  return html`
    ${renderAppList(app)}
    ${renderRefreshOptions(app)}
    ${renderJoyose(app)}
  `;
}
