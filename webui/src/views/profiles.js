import { html } from 'lit';
import { RISK, icon, when } from '../ui.js';

function summary(app, p) {
  if (p.id === 'stock') return ['不做任何修改'];
  const out = [];
  const names = Object.fromEntries(app.info.categories.map((c) => [c.id, c.name]));
  const describe = (map, prefix) => {
    const groups = {};
    for (const [id, r] of Object.entries(map ?? {})) {
      const n = names[id] ?? id;
      let label = null;
      if (r.mode === 'offset') label = [r.start ? `${r.start}°C 起控` : '', r.offset ? `${r.offset > 0 ? '+' : ''}${r.offset}°C` : ''].filter(Boolean).join('，') || null;
      if (r.mode === 'first') label = '仅留首档';
      if (r.mode === 'defer') label = r.value <= 20 ? '始终限制' : `${r.value}°C 起限制`;
      if (r.mode === 'curve') label = '内置曲线';
      if (r.mode === 'disable') label = '移除';
      if (label) (groups[label] ??= []).push(n);
    }
    for (const [label, list] of Object.entries(groups)) out.push(`${prefix}${list.join('、')} ${label}`);
  };
  describe(p.categories, '');
  describe(p.game, '游戏：');
  const floors = Object.values(p.cpuFloor ?? {}).filter(Boolean).length;
  if (floors) out.push(`${floors} 簇降频下限`);
  if (p.gpuMaxLevel != null) out.push(`GPU 等级 ≤ ${p.gpuMaxLevel}`);
  if (p.lock) out.push(`锁定场景`);
  const rules = Object.keys(p.rules ?? {}).length;
  if (rules) out.push(`${rules} 条规则微调`);
  const scenes = Object.values(p.scenes ?? {}).filter((s) => s.mode !== 'profile').length;
  if (scenes) out.push(`${scenes} 个场景单独设置`);
  return out.length ? out : ['与官方一致'];
}

/** 总开关与方案卡片（温控页） */
export function renderProfiles(app) {
  const st = app.status?.daemon.state;
  const list = app.allProfiles();
  return html`
    <div class="master card-filled ${app.cfg.enabled ? 'on' : ''}" @click=${() => app.toggleEnabled()}>
      <div class="master-icon">${icon(app.cfg.enabled ? 'check_circle' : 'power_settings_new')}</div>
      <div class="grow">
        <div class="master-title">${app.cfg.enabled ? '温控调度已启用' : '温控调度未启用'}</div>
        <div class="master-sub">${app.cfg.enabled ? '按方案选择顺序接管系统温控' : '当前使用系统官方温控'}</div>
      </div>
      <md-switch .selected=${app.cfg.enabled} @click=${(e) => e.stopPropagation()} @change=${(e) => app.toggleEnabled(e.target.selected)}></md-switch>
    </div>

    <div class="section-head"><h2>默认方案</h2><span class="hint-inline">轻触设为默认方案</span></div>
    <div class="profile-grid">
      ${list.map((p) => {
        const isDefault = app.cfg.active === p.id;
        const live = st?.effective === p.id && app.cfg.enabled;
        const risk = RISK[p.risk];
        return html`<article class="profile ${isDefault ? 'default' : ''}" @click=${() => app.setDefault(p.id)}>
          <div class="profile-top">
            <div class="avatar">${icon(p.icon || 'tune')}</div>
            <div class="grow">
              <div class="profile-name">${p.name}
                ${when(risk, () => html`<span class="tag ${risk.cls}">${risk.label}</span>`)}
                ${when(!p.builtin, () => html`<span class="tag">自定义</span>`)}
              </div>
              <div class="profile-flags">
                ${when(isDefault, () => html`<span class="flag">${icon('star')}默认</span>`)}
                ${when(live, () => html`<span class="flag live">${icon('play_arrow')}生效中</span>`)}
              </div>
            </div>
            ${when(p.id !== 'stock', () => html`<md-icon-button @click=${(e) => { e.stopPropagation(); app.profileMenu(p); }}>${icon('more_vert')}</md-icon-button>`)}
          </div>
          <p class="profile-desc">${p.desc || '无描述'}</p>
          <div class="profile-sum">${summary(app, p).map((t) => html`<span>${t}</span>`)}</div>
        </article>`;
      })}
    </div>
    <md-filled-tonal-button class="block-btn" @click=${() => app.newProfile()}><span slot="icon">${icon('add')}</span>新建方案</md-filled-tonal-button>
  `;
}
