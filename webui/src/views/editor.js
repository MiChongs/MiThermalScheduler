// 方案编辑器：基础 / 频率 / 场景 / 规则 / 预览
import { html } from 'lit';
import { previewProfile } from '../api.js';
import { CAT_ICONS, PROFILE_ICONS, cpuRange, icon, mhz, when } from '../ui.js';

const TABS = ['基础', '频率', '场景', '规则', '预览'];
const MODES = [
  ['keep', '保持官方'],
  ['first', '仅留首档'],
  ['defer', '推迟'],
  ['offset', '偏移'],
  ['curve', '替换曲线'],
  ['disable', '移除'],
];
const MODE_HINT = {
  keep: '使用官方规则，不做修改。',
  first: '保留最轻的一档限制，移除随温度升高的逐级降频。',
  defer: '合并为一档，到达指定温度才开始限制，限制强度取首档。',
  offset: '调整最高档的触发温度，正值推迟降频，负值提前降频；可设置起控温度推迟常驻限频。',
  curve: '移除官方规则，改用内置限流曲线。',
  disable: '移除该类全部规则。',
};
const CURVES = {
  charge: [['standard', '标准：37°C 起逐级限流'], ['gentle', '温和：限流更多，发热更低'], ['game', '游戏：边玩边充时持续轻度限流']],
  wireless: [['wireless', '无线充电：37°C 起逐级限流']],
};
const PROTECT = ['charge', 'warn'];

/** 分类策略列表；game 为 true 时用于游戏场景，可选择沿用日常场景的设置 */
function catRows(app, map, game) {
  const up = () => app.requestUpdate();
  return html`<div class="card-outlined">
    ${app.info.categories.map((c) => {
      const r = map[c.id];
      const mode = r?.mode ?? (game ? 'inherit' : 'keep');
      const modes = MODES.filter(([m]) => m !== 'curve' || CURVES[c.id]);
      const risky = PROTECT.includes(c.id);
      const set = async (m) => {
        if (m === 'inherit') { delete map[c.id]; up(); return; }
        if (m === 'disable' && risky && !(await app.ask({ title: `移除「${c.name}」规则？`, body: '该类规则用于安全保护。移除后高温时将不再限制，可能加速电池老化或导致机身过热。', ok: '移除', danger: true }))) { up(); return; }
        const n = (map[c.id] ??= { mode: 'keep', offset: 0, start: 0, value: 0, curve: '' });
        n.mode = m;
        if (m === 'offset' && !n.offset) n.offset = 3;
        if (m === 'defer' && !n.value) n.value = 60;
        if (m === 'curve' && !CURVES[c.id].some(([k]) => k === n.curve)) n.curve = CURVES[c.id][0][0];
        up();
      };
      return html`<div class="cat-row">
        <div class="cat-head">
          <div class="avatar small">${icon(CAT_ICONS[c.id] ?? 'tune')}</div>
          <div class="grow">
            <div class="row-title">${c.name}${when(risky, () => html`<span class="tag danger">保护</span>`)}</div>
            <div class="row-sub">${c.desc}</div>
          </div>
          <select class="native-select" .value=${mode} @change=${(e) => set(e.target.value)}>
            ${when(game, () => html`<option value="inherit" ?selected=${mode === 'inherit'}>沿用日常</option>`)}
            ${modes.map(([m, label]) => html`<option value=${m} ?selected=${mode === m}>${label}</option>`)}
          </select>
        </div>
        ${when(mode !== 'inherit', () => html`<div class="row-sub mode-hint">${MODE_HINT[mode]}${mode === 'first' && c.id === 'cpu' && !game ? '各场景统一采用日常配置的首档。' : ''}</div>`)}
        ${when(mode === 'offset', () => html`<div class="slider-row">
          <md-slider min="-10" max="20" step="1" labeled .value=${r.offset} @input=${(e) => { r.offset = e.target.value; up(); }}></md-slider>
          <span class="slider-val">${r.offset > 0 ? '+' : ''}${r.offset}°C</span>
        </div>
        ${when(!risky, () => html`<label class="start-row">
          <span>起控温度</span>
          <select class="native-select" .value=${String(r.start || 0)} @change=${(e) => { r.start = Number(e.target.value); up(); }}>
            <option value="0" ?selected=${!r.start}>不设置</option>
            ${[34, 36, 38, 40, 42, 44, 46].map((t) => html`<option value=${String(t)} ?selected=${r.start === t}>${t}°C</option>`)}
          </select>
        </label>`)}`)}
        ${when(mode === 'defer', () => html`<label class="start-row">
          <span>触发温度</span>
          <select class="native-select" .value=${String(r.value)} @change=${(e) => { r.value = Number(e.target.value); up(); }}>
            ${[15, 40, 42, 44, 46, 48, 50, 52, 55, 60].map((t) => html`<option value=${String(t)} ?selected=${r.value === t}>${t === 15 ? '15°C（始终限制）' : `${t}°C`}</option>`)}
          </select>
        </label>`)}
        ${when(mode === 'curve', () => html`<label class="start-row">
          <span>曲线</span>
          <select class="native-select" .value=${r.curve} @change=${(e) => { r.curve = e.target.value; up(); }}>
            ${CURVES[c.id].map(([k, label]) => html`<option value=${k} ?selected=${r.curve === k}>${label}</option>`)}
          </select>
        </label>`)}
      </div>`;
    })}
  </div>`;
}

const isTemp = (v) => /^\d+$/.test(v) && Number(v) >= 10000;
const toC = (v) => (isTemp(v) ? String(Number(v) / 1000) : v);
const fromC = (v) => {
  const n = Number(v);
  return Number.isFinite(n) && n > 0 && n < 200 ? String(Math.round(n * 1000)) : v;
};
const field = (sec, k) => sec.kv.find(([key]) => key === k)?.[1] ?? [];

export function renderEditor(app) {
  const ed = app.editor;
  return html`
    <md-tabs class="ed-tabs" .activeTabIndex=${ed.tab} @change=${(e) => { ed.tab = e.target.activeTabIndex; app.requestUpdate(); e.target.closest('.sheet-body')?.scrollTo({ top: 0 }); }}>
      ${TABS.map((t) => html`<md-primary-tab>${t}</md-primary-tab>`)}
    </md-tabs>
    <div class="ed-body">
      ${[basicTab, freqTab, sceneTab, rulesTab, previewTab][ed.tab](app, ed.draft)}
    </div>
  `;
}

function basicTab(app, d) {
  const up = () => app.requestUpdate();
  return html`
    <div class="card-outlined pad form">
      <md-outlined-text-field label="方案名称" .value=${d.name} maxlength="16" @input=${(e) => { d.name = e.target.value; up(); }}></md-outlined-text-field>
      <md-outlined-text-field label="描述" type="textarea" rows="2" .value=${d.desc} @input=${(e) => { d.desc = e.target.value; }}></md-outlined-text-field>
      <div class="field-label">图标</div>
      <div class="icon-pick">
        ${PROFILE_ICONS.map((n) => html`<button class=${d.icon === n ? 'on' : ''} @click=${() => { d.icon = n; up(); }}>${icon(n)}</button>`)}
      </div>
    </div>

    <div class="section-head"><h2>日常场景</h2></div>
    <p class="hint">作用于游戏与跑分以外的全部场景。启用过热保护时，推迟或偏移后的触发温度不超过保护触发温度（当前 ${app.cfg.guard.enabled ? `${app.cfg.guard.trip}°C` : '未启用'}），且不低于官方设置。传感器定义与低电量保护（BCL）始终保留。</p>
    ${catRows(app, d.categories, false)}

    <div class="section-head"><h2>游戏场景</h2></div>
    <p class="hint">作用于游戏、跑分与高帧率场景（系统识别到游戏时自动进入）。未单独设置的分类沿用日常场景的设置。</p>
    ${catRows(app, (d.game ??= {}), true)}
  `;
}

function freqTab(app, d) {
  const up = () => app.requestUpdate();
  const names = { 0: '小核', 2: '大核', 5: '大核', 7: '超大核' };
  const gpu = app.info.gpu;
  return html`
    <p class="hint">降频下限：温控降频时不会低于此频率。仅作用于 CPU 降频规则，不影响低电量保护。下限设置过高时，高温下将无法有效降温，请同时启用过热保护。</p>
    <div class="card-outlined pad form">
      ${app.info.clusters.map((c) => {
        const key = `cpu${c.first}`;
        const val = String(d.cpuFloor[key] ?? 0);
        return html`<md-outlined-select label="${names[c.first] ?? c.policy}（CPU ${cpuRange(c.cpus)}）降频下限" .value=${val}
          @change=${(e) => { const v = Number(e.target.value); if (v) d.cpuFloor[key] = v; else delete d.cpuFloor[key]; up(); }}>
          <md-select-option value="0" ?selected=${val === '0'}><div slot="headline">不限制</div></md-select-option>
          ${[...c.freqs].reverse().map((f) => html`<md-select-option value=${String(f)} ?selected=${val === String(f)}><div slot="headline">${mhz(f)} MHz</div></md-select-option>`)}
        </md-outlined-select>`;
      })}
    </div>
    <div class="section-head"><h2>GPU</h2></div>
    <p class="hint">GPU 温控以等级限制最高频率，等级越大频率越低。设置上限后，温控降频不会超过该等级。</p>
    <div class="card-outlined pad form">
      <md-outlined-select label="GPU 最大降频等级" .value=${d.gpuMaxLevel == null ? '-1' : String(d.gpuMaxLevel)}
        @change=${(e) => { const v = Number(e.target.value); d.gpuMaxLevel = v < 0 ? null : v; up(); }}>
        <md-select-option value="-1" ?selected=${d.gpuMaxLevel == null}><div slot="headline">不限制</div></md-select-option>
        ${gpu.freqs.map((f, i) => html`<md-select-option value=${String(i)} ?selected=${d.gpuMaxLevel === i}><div slot="headline">等级 ${i}（${Math.round(f / 1e6)} MHz）</div></md-select-option>`)}
      </md-outlined-select>
    </div>
  `;
}

function sceneTab(app, d) {
  const up = () => app.requestUpdate();
  const scenes = app.scenes;
  if (!scenes) return html`<div class="placeholder">正在读取场景配置…</div>`;
  return html`
    <div class="card-outlined pad form">
      <md-outlined-select label="锁定场景" .value=${d.lock} @change=${(e) => { d.lock = e.target.value; up(); }}>
        <md-select-option value="" ?selected=${!d.lock}><div slot="headline">不锁定（跟随系统场景）</div></md-select-option>
        ${scenes.list.map((s) => html`<md-select-option value=${s.file} ?selected=${d.lock === s.file}><div slot="headline">${s.label}</div><div slot="supporting-text">${s.file}</div></md-select-option>`)}
      </md-outlined-select>
      <p class="hint flat">锁定后，所有场景均使用所选场景的策略，传感器定义保持不变。</p>
    </div>
    <div class="section-head"><h2>逐场景设置</h2></div>
    <div class="card-outlined">
      ${scenes.list.map((s) => {
        const o = d.scenes[s.file];
        const mode = o?.mode ?? 'profile';
        return html`<div class="list-row">
          <div class="grow">
            <div class="row-title">${s.label}</div>
            <div class="row-sub mono">${s.file}${mode === 'text' ? '（自定义全文）' : mode === 'stock' ? '（保持官方）' : ''}</div>
          </div>
          ${when(mode === 'text', () => html`<md-icon-button @click=${() => app.editSceneText(d, s)}>${icon('edit')}</md-icon-button>`)}
          <select class="native-select" .value=${mode} @change=${(e) => {
            const m = e.target.value;
            if (m === 'profile') delete d.scenes[s.file];
            else d.scenes[s.file] = { mode: m, text: o?.text || (m === 'text' ? s.text : '') };
            up();
            if (m === 'text') app.editSceneText(d, s);
          }}>
            <option value="profile">跟随方案</option>
            <option value="stock">保持官方</option>
            <option value="text">自定义全文</option>
          </select>
        </div>`;
      })}
    </div>
  `;
}

function ruleSummary(sec) {
  const trig = field(sec, 'trig');
  const temps = trig.filter(isTemp).map((v) => Number(v) / 1000);
  const range = temps.length ? `${Math.min(...temps)}–${Math.max(...temps)}°C` : trig.join(' ');
  return `${field(sec, 'device').join('')}，${trig.length} 档，${range}`;
}

function rulesTab(app, d) {
  const scenes = app.scenes;
  if (!scenes) return html`<div class="placeholder">正在读取场景配置…</div>`;
  const ed = app.editor;
  const file = ed.ruleScene ?? '*';
  const src = scenes.byFile[file === '*' ? 'thermal-normal.conf' : file];
  const secs = src.sections.filter((s) => s.category !== 'sensor' && s.category !== 'bcl');
  const catName = Object.fromEntries(app.info.categories.map((c) => [c.id, c.name]));
  return html`
    <p class="hint">可逐条修改规则的触发温度、解除温度与目标值，或停用指定规则。选择「所有场景」时，修改作用于各场景中的同名规则，并以日常场景为模板。</p>
    <md-outlined-select class="full" label="场景" .value=${file} @change=${(e) => { ed.ruleScene = e.target.value; app.requestUpdate(); }}>
      <md-select-option value="*" ?selected=${file === '*'}><div slot="headline">所有场景</div></md-select-option>
      ${scenes.list.map((s) => html`<md-select-option value=${s.file} ?selected=${file === s.file}><div slot="headline">${s.label}</div><div slot="supporting-text">${s.file}</div></md-select-option>`)}
    </md-outlined-select>
    <div class="card-outlined">
      ${secs.map((sec) => {
        const key = `${file}#${sec.base}`;
        const o = d.rules[key];
        const inherited = file !== '*' && !o && d.rules[`*#${sec.base}`];
        return html`<div class="list-row clickable" @click=${() => app.editRule(d, file, sec)}>
          <div class="avatar small">${icon(CAT_ICONS[sec.category])}</div>
          <div class="grow">
            <div class="row-title mono">${sec.name}</div>
            <div class="row-sub">${catName[sec.category] ?? sec.category}：${ruleSummary(sec)}</div>
          </div>
          ${o ? html`<span class="tag ${o.disabled ? 'danger' : 'accent'}">${o.disabled ? '已停用' : '已微调'}</span>` : inherited ? html`<span class="tag">继承</span>` : ''}
          ${icon('chevron_right', 'dim')}
        </div>`;
      })}
    </div>
  `;
}

/** 规则编辑面板 */
export function ruleSheet(app, d, file, sec) {
  const key = `${file}#${sec.base}`;
  const base = d.rules[key] ?? {};
  const trig = base.trig ?? field(sec, 'trig');
  const clr = base.clr ?? field(sec, 'clr');
  const target = base.target ?? field(sec, 'target');
  const n = Math.max(trig.length, clr.length, target.length);
  const state = {
    disabled: !!base.disabled,
    polling: base.polling ?? field(sec, 'polling')[0] ?? '',
    rows: Array.from({ length: n }, (_, i) => ({ trig: toC(trig[i] ?? ''), clr: toC(clr[i] ?? ''), target: target[i] ?? '' })),
  };
  const meta = [
    ['算法', field(sec, 'algo_type').join(' ')],
    ['传感器', field(sec, 'sensor').join(' ')],
    ['设备', field(sec, 'device').join(' ')],
  ];
  const render = () => html`
    <div class="card-outlined pad kv3">
      ${meta.map(([k, v]) => html`<div><span>${k}</span><b class="mono">${v || '-'}</b></div>`)}
    </div>
    <label class="list-row card-outlined switch-row">
      <div class="grow"><div class="row-title">启用此规则</div><div class="row-sub">关闭后将从${file === '*' ? '所有场景' : '该场景'}中移除此规则</div></div>
      <md-switch .selected=${!state.disabled} @change=${(e) => { state.disabled = !e.target.selected; app.requestUpdate(); }}></md-switch>
    </label>
    ${when(!state.disabled, () => html`
      <div class="card-outlined pad form">
        <md-outlined-text-field label="轮询间隔 (ms)" type="number" .value=${state.polling} @input=${(e) => { state.polling = e.target.value; }}></md-outlined-text-field>
      </div>
      <div class="section-head"><h2>档位</h2><span class="hint-inline">温度单位 °C</span></div>
      <div class="level-table">
        <div class="lt-head"><span>#</span><span>触发</span><span>解除</span><span>目标</span><span></span></div>
        ${state.rows.map((r, i) => html`<div class="lt-row">
          <span class="lt-idx">${i + 1}</span>
          <input inputmode="decimal" .value=${r.trig} @input=${(e) => { r.trig = e.target.value; }}>
          <input inputmode="decimal" .value=${r.clr} @input=${(e) => { r.clr = e.target.value; }}>
          <input .value=${r.target} @input=${(e) => { r.target = e.target.value; }}>
          <md-icon-button @click=${() => { state.rows.splice(i, 1); app.requestUpdate(); }}>${icon('close')}</md-icon-button>
        </div>`)}
        <md-text-button @click=${() => { const last = state.rows.at(-1) ?? { trig: '', clr: '', target: '' }; state.rows.push({ ...last }); app.requestUpdate(); }}>
          <span slot="icon">${icon('add')}</span>添加档位
        </md-text-button>
      </div>
      <p class="hint">目标值的含义取决于设备：CPU 为频率（kHz），GPU 与 NPU 为限制等级，充电为电流等级；多个设备的目标值以「+」连接。</p>
    `)}
  `;
  const save = () => {
    const o = { disabled: state.disabled };
    if (!state.disabled) {
      const rows = state.rows.filter((r) => r.trig !== '' || r.target !== '');
      const t = rows.map((r) => fromC(r.trig));
      const c = rows.map((r) => fromC(r.clr));
      const g = rows.map((r) => r.target);
      const same = (a, b) => a.length === b.length && a.every((v, i) => v === b[i]);
      if (!same(t, field(sec, 'trig'))) o.trig = t;
      if (!same(c, field(sec, 'clr'))) o.clr = c;
      if (!same(g, field(sec, 'target'))) o.target = g;
      if (state.polling !== (field(sec, 'polling')[0] ?? '')) o.polling = state.polling;
    }
    if (Object.keys(o).length === 1 && !o.disabled) delete d.rules[key];
    else d.rules[key] = o;
    return true;
  };
  const reset = () => {
    delete d.rules[key];
    return true;
  };
  return { title: sec.name, render, save, extra: { label: '恢复默认', run: reset } };
}

function previewTab(app, d) {
  const ed = app.editor;
  const p = ed.preview;
  return html`
    <p class="hint">按当前编辑内容（未保存）生成各场景的配置，并与官方配置对比。</p>
    <md-filled-tonal-button @click=${async () => {
      ed.preview = { loading: true };
      app.requestUpdate();
      try {
        ed.preview = await previewProfile(d);
      } catch (e) {
        ed.preview = { error: e.message };
      }
      app.requestUpdate();
    }}><span slot="icon">${icon('visibility')}</span>生成预览</md-filled-tonal-button>
    ${when(p?.loading, () => html`<div class="placeholder"><md-circular-progress indeterminate></md-circular-progress></div>`)}
    ${when(p?.error, () => html`<p class="error-text">${p.error}</p>`)}
    ${when(p?.scenes, () => html`
      <div class="section-head"><h2>${p.changed} / ${p.scenes.length} 个场景将被覆盖</h2></div>
      <div class="card-outlined">
        ${p.scenes.map((s) => html`<div class="list-row clickable" @click=${() => app.viewGenerated(d, s)}>
          <div class="grow"><div class="row-title">${s.label}</div><div class="row-sub mono">${s.file}</div></div>
          <span class="tag ${s.changed ? 'accent' : ''}">${s.changed ? `已修改（${s.before} 段变为 ${s.after} 段）` : '官方'}</span>
          ${icon('chevron_right', 'dim')}
        </div>`)}
      </div>
    `)}
  `;
}
