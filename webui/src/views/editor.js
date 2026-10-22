// 方案编辑器：基础 / 频率 / 场景 / 规则 / 预览
import { html } from 'lit';
import { previewProfile } from '../api.js';
import { CAT_ICONS, PROFILE_ICONS, cpuRange, icon, mhz, when } from '../ui.js';

const TABS = ['基础', '频率', '场景', '规则', '预览'];
const MODES = [
  ['keep', '保持'],
  ['offset', '偏移'],
  ['disable', '移除'],
];

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

    <div class="section-head"><h2>分类策略</h2></div>
    <p class="hint">「偏移」整体调整该类规则的触发 / 解除温度：正值更晚降频，负值更早降频。传感器定义与低电量保护（BCL）始终保留。</p>
    <div class="card-outlined">
      ${app.info.categories.map((c) => {
        const r = (d.categories[c.id] ??= { mode: 'keep', offset: 0 });
        const risky = c.id === 'charge' || c.id === 'warn';
        return html`<div class="cat-row">
          <div class="cat-head">
            <div class="avatar small">${icon(CAT_ICONS[c.id])}</div>
            <div class="grow">
              <div class="row-title">${c.name}${when(risky, () => html`<span class="tag danger">保护</span>`)}</div>
              <div class="row-sub">${c.desc}</div>
            </div>
          </div>
          <div class="seg">
            ${MODES.map(([m, label]) => html`<button class=${r.mode === m ? 'on' : ''} @click=${async () => {
              if (m === 'disable' && risky && !(await app.ask({ title: `移除「${c.name}」规则？`, body: '该类规则用于安全保护。移除后高温时将不再限制，可能加速电池老化或导致机身过热。', ok: '移除', danger: true }))) return;
              r.mode = m;
              if (m === 'offset' && !r.offset) r.offset = 3;
              up();
            }}>${r.mode === m ? icon('check') : ''}${label}</button>`)}
          </div>
          ${when(r.mode === 'offset', () => html`<div class="slider-row">
            <md-slider min="-10" max="20" step="1" labeled .value=${r.offset} @input=${(e) => { r.offset = e.target.value; up(); }}></md-slider>
            <span class="slider-val">${r.offset > 0 ? '+' : ''}${r.offset}°C</span>
          </div>`)}
        </div>`;
      })}
    </div>
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
