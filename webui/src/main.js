import '@material/web/button/filled-button.js';
import '@material/web/button/filled-tonal-button.js';
import '@material/web/button/outlined-button.js';
import '@material/web/button/text-button.js';
import '@material/web/iconbutton/icon-button.js';
import '@material/web/fab/fab.js';
import '@material/web/switch/switch.js';
import '@material/web/slider/slider.js';
import '@material/web/select/outlined-select.js';
import '@material/web/select/select-option.js';
import '@material/web/textfield/outlined-text-field.js';
import '@material/web/checkbox/checkbox.js';
import '@material/web/dialog/dialog.js';
import '@material/web/tabs/tabs.js';
import '@material/web/tabs/primary-tab.js';
import '@material/web/progress/circular-progress.js';
import '@material/web/labs/navigationbar/navigation-bar.js';
import '@material/web/labs/navigationtab/navigation-tab.js';

import { LitElement, html } from 'lit';
import { BIN, ctl, exec, hasKsu, listApps, previewProfile, saveConfig } from './api.js';
import { initTheme } from './theme.js';
import { TRIGGERS, clone, icon, uid, when } from './ui.js';
import { renderAuto } from './views/auto.js';
import { renderEditor, ruleSheet } from './views/editor.js';
import { renderHome } from './views/home.js';
import { renderLogs } from './views/logs.js';
import { renderProfiles } from './views/profiles.js';
import { renderSettings } from './views/settings.js';

const PAGES = [
  { id: 'home', label: '概览', icon: 'dashboard', render: renderHome },
  { id: 'profiles', label: '方案', icon: 'tune', render: renderProfiles },
  { id: 'auto', label: '自动化', icon: 'automation', render: renderAuto },
  { id: 'logs', label: '日志', icon: 'history', render: renderLogs },
  { id: 'settings', label: '设置', icon: 'settings', render: renderSettings },
];
const HISTORY = 90;

class App extends LitElement {
  static properties = {
    page: { state: true },
    status: { state: true },
    sheets: { state: true },
    dlg: { state: true },
    snack: { state: true },
    busy: { state: true },
    fatal: { state: true },
  };

  constructor() {
    super();
    this.page = 'home';
    this.sheets = [];
    this.hist = { board: [], cpu: [] };
    this.logTab = 'thermal';
    this.logFilter = 'all';
    this.labels = new Map();
    this.busy = '正在连接后端…';
  }

  createRenderRoot() {
    return this;
  }

  async connectedCallback() {
    super.connectedCallback();
    await initTheme();
    if (!hasKsu()) {
      this.busy = '';
      this.fatal = '请在 KernelSU 管理器中打开此 WebUI。';
      return;
    }
    try {
      this.info = await ctl('info');
      this.cfg = clone(this.info.config);
      await this.poll();
      this.loadScenes();
      listApps().then((apps) => { this.apps = apps; for (const a of apps) this.labels.set(a.pkg, a.label); this.requestUpdate(); });
    } catch (e) {
      this.fatal = `无法连接后端：${e.message}`;
    }
    this.busy = '';
    document.addEventListener('visibilitychange', () => this.schedule());
    window.addEventListener('scroll', () => this.querySelector('.appbar')?.classList.toggle('lifted', window.scrollY > 4), { passive: true });
  }

  // ---------- 数据 ----------
  async poll() {
    clearTimeout(this.timer);
    try {
      const s = await ctl('status');
      this.status = s;
      const cpu = s.temps.find((t) => t.id === 'cpu')?.value;
      this.hist.board = [...this.hist.board, s.board].slice(-HISTORY);
      if (Number.isFinite(cpu)) this.hist.cpu = [...this.hist.cpu, cpu].slice(-HISTORY);
      if (this.page === 'logs') await this.loadLogs();
    } catch {
      // 偶发失败时保持上一份状态
    }
    this.schedule();
  }

  schedule() {
    clearTimeout(this.timer);
    if (!document.hidden && this.info) this.timer = setTimeout(() => this.poll(), 2000);
  }

  async loadScenes() {
    if (this.scenes) return;
    const data = await ctl('scenes');
    const list = data.scenes.sort((a, b) => (a.file === 'thermal-normal.conf' ? -1 : b.file === 'thermal-normal.conf' ? 1 : a.label.localeCompare(b.label, 'zh')));
    this.scenes = { list, byFile: Object.fromEntries(list.map((s) => [s.file, s])), map: data.map };
    this.requestUpdate();
  }

  async loadLogs() {
    this.logs = await ctl('log -n 200');
    this.requestUpdate();
  }

  setLogTab(t) {
    this.logTab = t;
    this.requestUpdate();
  }

  allProfiles() {
    return [...this.info.builtins, ...this.cfg.profiles.map((p) => ({ ...p, builtin: false }))];
  }

  findProfile(id) {
    return this.allProfiles().find((p) => p.id === id);
  }

  profileIcon(id) {
    return this.findProfile(id)?.icon || 'tune';
  }

  appLabel(pkg) {
    return this.labels.get(pkg) ?? pkg;
  }

  /** 修改配置并保存，失败时回滚 */
  async edit(mutate, msg) {
    const before = clone(this.cfg);
    mutate(this.cfg);
    this.requestUpdate();
    try {
      const r = await saveConfig(this.cfg);
      if (r.warning) this.toast(`已保存，但应用失败：${r.warning}`);
      else if (msg) this.toast(msg);
      setTimeout(() => this.poll(), 300);
      return true;
    } catch (e) {
      this.cfg = before;
      this.requestUpdate();
      await this.ask({ title: '保存失败', body: e.message, ok: '知道了', cancel: null });
      return false;
    }
  }

  // ---------- 方案 ----------
  async setDefault(id) {
    const p = this.findProfile(id);
    if (id !== 'stock' && p.risk >= 2 && !(await this.ask({ title: `启用「${p.name}」？`, body: `${p.desc}\n\n此方案会显著放宽温控限制，${this.cfg.guard.enabled ? `过热保护将在 ${this.cfg.guard.trip}°C 时介入。` : '且过热保护当前已关闭，请谨慎使用。'}`, ok: '启用', danger: true }))) return;
    await this.edit((c) => {
      if (id === 'stock') c.enabled = false;
      else {
        c.enabled = true;
        c.active = id;
      }
    }, id === 'stock' ? '已恢复官方温控' : `默认方案：${p.name}`);
  }

  quickSwitch(id) {
    return this.setDefault(id);
  }

  toggleEnabled(v) {
    const on = v ?? !this.cfg.enabled;
    if (on && this.cfg.active === 'stock') this.cfg.active = 'balanced';
    return this.edit((c) => { c.enabled = on; }, on ? '温控调度已启用' : '已恢复官方温控');
  }

  async profileMenu(p) {
    const opts = [];
    if (!p.builtin) opts.push({ value: 'edit', label: '编辑', icon: 'edit' });
    opts.push({ value: 'copy', label: p.builtin ? '基于此方案新建' : '复制', icon: 'content_copy' });
    if (!p.builtin) opts.push({ value: 'delete', label: '删除', icon: 'delete', danger: true });
    const v = await this.choose(p.name, opts);
    if (v === 'edit') this.openEditor(p, false);
    if (v === 'copy') this.openEditor({ ...clone(p), id: uid('p'), name: `${p.name} 副本`, builtin: false }, true);
    if (v === 'delete') {
      const used = this.cfg.rules.filter((r) => r.profile === p.id).length;
      if (!(await this.ask({ title: `删除「${p.name}」？`, body: used ? `有 ${used} 条自动化规则使用此方案，这些规则也会被删除。` : '此操作无法撤销。', ok: '删除', danger: true }))) return;
      await this.edit((c) => {
        c.profiles = c.profiles.filter((x) => x.id !== p.id);
        c.rules = c.rules.filter((r) => r.profile !== p.id);
        if (c.active === p.id) c.active = 'balanced';
      }, '已删除');
    }
  }

  newProfile() {
    this.openEditor({ id: uid('p'), name: '新方案', desc: '', icon: 'tune', risk: 0, lock: '', categories: {}, cpuFloor: {}, gpuMaxLevel: null, rules: {}, scenes: {} }, true);
  }

  openEditor(profile, isNew) {
    const draft = clone(profile);
    for (const k of ['categories', 'cpuFloor', 'rules', 'scenes']) draft[k] ??= {};
    this.editor = { draft, tab: 0, isNew };
    this.loadScenes();
    this.pushSheet({
      title: isNew ? '新建方案' : `编辑「${profile.name}」`,
      wide: true,
      render: () => renderEditor(this),
      save: () => this.saveDraft(),
      onClose: () => { this.editor = null; },
    });
  }

  async saveDraft() {
    const d = this.editor.draft;
    if (!d.name.trim()) {
      await this.ask({ title: '请填写方案名称', ok: '好', cancel: null });
      return false;
    }
    const disabled = Object.entries(d.categories).filter(([, r]) => r.mode === 'disable').map(([k]) => k);
    d.risk = disabled.some((k) => ['cpu', 'gpu', 'charge', 'warn'].includes(k)) ? 2 : disabled.length || Object.keys(d.cpuFloor).length || Object.values(d.categories).some((r) => r.mode === 'offset' && r.offset >= 10) ? 1 : 0;
    for (const [k, r] of Object.entries(d.categories)) if (r.mode === 'keep') delete d.categories[k];
    delete d.builtin;
    const ok = await this.edit((c) => {
      const i = c.profiles.findIndex((p) => p.id === d.id);
      if (i >= 0) c.profiles[i] = d;
      else c.profiles.push(d);
    }, `已保存「${d.name}」`);
    return ok;
  }

  editSceneText(d, s) {
    const o = d.scenes[s.file];
    let value = o.text || s.text;
    this.pushSheet({
      title: s.label,
      render: () => html`<p class="hint">完整替换「${s.file}」的内容。保存方案后生效。</p>
        <textarea class="code" spellcheck="false" .value=${value} @input=${(e) => { value = e.target.value; }}></textarea>`,
      save: () => {
        if (!/^\s*\[/.test(value)) {
          this.ask({ title: '内容无效', body: '配置需以 [段名] 开头。', ok: '好', cancel: null });
          return false;
        }
        o.text = value;
        return true;
      },
      extra: { label: '载入官方', run: () => { value = s.text; this.requestUpdate(); return false; } },
    });
  }

  editRule(d, file, sec) {
    this.pushSheet(ruleSheet(this, d, file, sec));
  }

  async viewGenerated(d, s) {
    const r = await previewProfile(d, s.file);
    this.pushSheet({ title: s.label, render: () => html`<textarea class="code" readonly .value=${r.text}></textarea>` });
  }

  // ---------- 自动化 ----------
  editAutoRule(rule) {
    const r = rule ? clone(rule) : { id: uid('r'), enabled: true, kind: 'app', packages: [], value: 20, profile: 'gaming' };
    const picker = { query: '' };
    const up = () => this.requestUpdate();
    const profiles = this.allProfiles();
    this.pushSheet({
      title: rule ? '编辑规则' : '添加规则',
      render: () => {
        return html`
          <div class="section-head first"><h2>条件</h2></div>
          <div class="trigger-grid">
            ${Object.entries(TRIGGERS).map(([k, t]) => html`<button class="trigger ${r.kind === k ? 'on' : ''}" @click=${() => {
              r.kind = k;
              if (k === 'batteryBelow' && !(r.value > 0 && r.value <= 100)) r.value = 20;
              if (k === 'tempAbove' && !(r.value >= 30)) r.value = 45;
              up();
            }}>${icon(t.icon)}<span>${t.name}</span></button>`)}
          </div>
          ${r.kind === 'batteryBelow' ? html`<div class="card-outlined pad slider-block">
              <div class="slider-label"><span>电量低于</span><b>${r.value}%</b></div>
              <md-slider min="5" max="80" step="5" labeled .value=${r.value} @input=${(e) => { r.value = e.target.value; up(); }}></md-slider>
            </div>` : ''}
          ${r.kind === 'tempAbove' ? html`<div class="card-outlined pad slider-block">
              <div class="slider-label"><span>主板温度达到</span><b>${r.value}°C</b></div>
              <md-slider min="35" max="55" step="1" labeled .value=${r.value} @input=${(e) => { r.value = e.target.value; up(); }}></md-slider>
            </div>` : ''}

          <div class="section-head"><h2>切换到方案</h2></div>
          <div class="chip-scroll wrap">
            ${profiles.map((p) => html`<button class="pchip ${r.profile === p.id ? 'on' : ''}" @click=${() => { r.profile = p.id; up(); }}>${icon(p.icon || 'tune')}<span>${p.name}</span></button>`)}
          </div>

          ${r.kind === 'app' ? this.appPicker(r, picker) : ''}
        `;
      },
      save: async () => {
        if (r.kind === 'app' && !r.packages.length) {
          await this.ask({ title: '请至少选择一个应用', ok: '好', cancel: null });
          return false;
        }
        return this.edit((c) => {
          const i = c.rules.findIndex((x) => x.id === r.id);
          if (i >= 0) c.rules[i] = r;
          else c.rules.push(r);
          c.automation = true;
        }, '规则已保存');
      },
    });
  }

  /** 应用多选列表；target.packages 为已选包名，picker.query 为搜索词 */
  appPicker(target, picker) {
    const up = () => this.requestUpdate();
    const q = picker.query;
    const apps = (this.apps ?? []).filter((a) => !q || a.label.toLowerCase().includes(q) || a.pkg.includes(q));
    const picked = new Set(target.packages);
    return html`
      <div class="section-head"><h2>应用</h2><span class="hint-inline">已选 ${target.packages.length} 个</span></div>
      <div class="search">${icon('search')}<input placeholder="搜索应用名称或包名" @input=${(e) => { picker.query = e.target.value.toLowerCase(); up(); }}></div>
      <div class="card-outlined app-list">
        ${!this.apps ? html`<div class="placeholder">正在读取应用列表…</div>` : apps.slice(0, 300).map((a) => html`<label class="list-row clickable">
          <div class="letter">${a.label.slice(0, 1)}</div>
          <div class="grow"><div class="row-title">${a.label}</div><div class="row-sub mono">${a.pkg}</div></div>
          <md-checkbox .checked=${picked.has(a.pkg)} @change=${(e) => {
            if (e.target.checked) target.packages = [...target.packages, a.pkg];
            else target.packages = target.packages.filter((x) => x !== a.pkg);
            up();
          }}></md-checkbox>
        </label>`)}
      </div>`;
  }

  // ---------- 应用刷新率 ----------
  editRefreshRule(rule) {
    const rates = this.info.refreshRates?.length ? this.info.refreshRates : [120, 90, 60];
    const r = rule ? clone(rule) : { id: uid('f'), enabled: true, packages: [], rate: rates[0] };
    const picker = { query: '' };
    const up = () => this.requestUpdate();
    this.pushSheet({
      title: rule ? '编辑刷新率规则' : '添加刷新率规则',
      render: () => html`
        <div class="section-head first"><h2>刷新率上限</h2></div>
        <div class="chip-scroll wrap">
          ${rates.map((hz) => html`<button class="pchip ${r.rate === hz ? 'on' : ''}" @click=${() => { r.rate = hz; up(); }}>${icon('speed')}<span>${hz} Hz</span></button>`)}
        </div>
        <p class="hint">所选应用位于前台时，系统刷新率上限锁定为该值。游戏内帧率选项不应超过屏幕支持的最高刷新率。</p>
        ${this.appPicker(r, picker)}
      `,
      save: async () => {
        if (!r.packages.length) {
          await this.ask({ title: '请至少选择一个应用', ok: '好', cancel: null });
          return false;
        }
        return this.edit((c) => {
          c.refresh ??= { enabled: true, respectGuard: true, rules: [] };
          const i = c.refresh.rules.findIndex((x) => x.id === r.id);
          if (i >= 0) c.refresh.rules[i] = r;
          else c.refresh.rules.push(r);
          c.refresh.enabled = true;
        }, '刷新率规则已保存');
      },
    });
  }

  async deleteRefreshRule(i) {
    if (await this.ask({ title: '删除这条刷新率规则？', ok: '删除', danger: true })) this.edit((c) => { c.refresh.rules.splice(i, 1); }, '已删除');
  }

  async deleteAutoRule(i) {
    if (await this.ask({ title: '删除这条规则？', ok: '删除', danger: true })) this.edit((c) => { c.rules.splice(i, 1); }, '已删除');
  }

  // ---------- 设置 ----------
  async restartDaemon() {
    this.busy = '正在重启守护进程…';
    await exec(`P=$(cat /data/adb/mi_thermal_m3/daemon.pid 2>/dev/null); [ -n "$P" ] && kill $P; sleep 1; setsid nohup ${BIN} daemon >/dev/null 2>&1 < /dev/null &`);
    await new Promise((r) => setTimeout(r, 2500));
    await this.poll();
    this.busy = '';
    this.toast(this.status?.daemon.running ? '守护进程已启动' : '守护进程启动失败，请查看模块日志');
  }

  async reapply() {
    this.busy = '正在应用…';
    try {
      await ctl('apply');
      this.toast('已重新应用');
    } catch (e) {
      await this.ask({ title: '应用失败', body: e.message, ok: '知道了', cancel: null });
    }
    this.busy = '';
    this.poll();
  }

  async resetAll() {
    if (!(await this.ask({ title: '恢复官方温控？', body: '将关闭总开关并删除所有覆盖文件。你的方案与自动化规则会保留。', ok: '恢复', danger: true }))) return;
    this.busy = '正在恢复…';
    try {
      await ctl('reset');
      this.cfg.enabled = false;
      this.toast('已恢复官方温控');
    } catch (e) {
      await this.ask({ title: '失败', body: e.message, ok: '知道了', cancel: null });
    }
    this.busy = '';
    this.poll();
  }

  exportConfig() {
    const text = JSON.stringify(this.cfg, null, 2);
    this.pushSheet({
      title: '导出配置',
      render: () => html`<textarea class="code" readonly .value=${text}></textarea>`,
      extra: { label: '复制', run: () => { navigator.clipboard?.writeText(text).then(() => this.toast('已复制')); return false; } },
    });
  }

  importConfig() {
    let value = '';
    this.pushSheet({
      title: '导入配置',
      render: () => html`<p class="hint">粘贴之前导出的 JSON，将覆盖当前全部方案与规则。</p>
        <textarea class="code" spellcheck="false" placeholder="{ ... }" @input=${(e) => { value = e.target.value; }}></textarea>`,
      saveLabel: '导入',
      save: async () => {
        let parsed;
        try {
          parsed = JSON.parse(value);
        } catch {
          await this.ask({ title: 'JSON 格式错误', ok: '好', cancel: null });
          return false;
        }
        return this.edit((c) => { Object.assign(c, parsed, { version: 2 }); }, '配置已导入');
      },
    });
  }

  async viewLoaded() {
    const r = await ctl('loaded');
    this.pushSheet({ title: `当前加载 · ${r.label}`, render: () => html`<p class="hint">sconfig = ${r.sconfig}，${r.scene}</p><textarea class="code" readonly .value=${r.text}></textarea>` });
  }

  // ---------- 面板、对话框与提示 ----------
  pushSheet(sheet) {
    this.sheets = [...this.sheets, sheet];
    document.body.classList.add('noscroll');
  }

  closeSheet() {
    const top = this.sheets.at(-1);
    top?.onClose?.();
    this.sheets = this.sheets.slice(0, -1);
    if (!this.sheets.length) document.body.classList.remove('noscroll');
  }

  async saveSheet(sheet) {
    const ok = await sheet.save();
    if (ok !== false) this.closeSheet();
  }

  ask({ title, body = '', ok = '确定', cancel = '取消', danger = false }) {
    return new Promise((resolve) => {
      this.dlg = { title, body, actions: [cancel && { label: cancel, value: false }, { label: ok, value: true, primary: true, danger }].filter(Boolean), resolve };
    });
  }

  choose(title, options) {
    return new Promise((resolve) => {
      this.dlg = { title, options, actions: [{ label: '取消', value: null }], resolve };
    });
  }

  finishDialog(value) {
    const d = this.dlg;
    this.dlg = null;
    d?.resolve(value);
  }

  toast(msg) {
    this.snack = msg;
    clearTimeout(this.snackTimer);
    this.snackTimer = setTimeout(() => { this.snack = ''; }, 2800);
  }

  setPage(id) {
    this.page = id;
    window.scrollTo({ top: 0 });
    if (id === 'logs') this.loadLogs();
    if (id === 'auto') this.loadJoyose();
  }

  // ---------- Joyose ----------
  async loadJoyose(delay = 0) {
    if (delay) await new Promise((r) => setTimeout(r, delay));
    try {
      this.joyose = await ctl('joyose status');
    } catch (e) {
      this.joyose = { installed: false, error: e.message };
    }
    this.requestUpdate();
  }

  async joyoseApply() {
    this.busy = '正在应用 Joyose 调教…';
    try {
      const r = await ctl('joyose apply');
      this.joyose = r.status;
      this.toast(r.result ?? '已是最新，无需重新应用');
    } catch (e) {
      await this.ask({ title: '应用失败', body: e.message, ok: '知道了', cancel: null });
    }
    this.busy = '';
  }

  // ---------- 渲染 ----------
  render() {
    if (this.fatal) return html`<div class="fatal">${icon('warning', 'big')}<p>${this.fatal}</p></div>`;
    const page = PAGES.find((p) => p.id === this.page);
    const ready = this.info && this.cfg;
    return html`
      <header class="appbar">
        <div class="grow">
          <h1>${page.id === 'home' ? '小米温控调度' : page.label}</h1>
          ${when(page.id === 'home' && this.info, () => html`<div class="appbar-sub">${this.info.device.model} · ${this.info.device.soc} · ${this.info.device.os}</div>`)}
        </div>
        <md-icon-button @click=${() => this.poll()}>${icon('restart_alt')}</md-icon-button>
      </header>
      <main class="page" data-page=${page.id}>${ready ? page.render(this) : ''}</main>

      <md-navigation-bar .activeIndex=${PAGES.indexOf(page)} @navigation-bar-activated=${(e) => this.setPage(PAGES[e.detail.activeIndex].id)}>
        ${PAGES.map((p) => html`<md-navigation-tab label=${p.label}>
          <span slot="active-icon">${icon(p.icon)}</span><span slot="inactive-icon">${icon(p.icon)}</span>
        </md-navigation-tab>`)}
      </md-navigation-bar>

      ${this.sheets.map((s, i) => html`<section class="sheet ${s.wide ? 'wide' : ''}" style="z-index:${30 + i}">
        <header class="sheet-bar">
          <md-icon-button @click=${() => this.closeSheet()}>${icon(s.save ? 'close' : 'arrow_back')}</md-icon-button>
          <h2 class="grow">${s.title}</h2>
          ${when(s.extra, () => html`<md-text-button @click=${async () => { if ((await s.extra.run()) !== false) this.closeSheet(); }}>${s.extra.label}</md-text-button>`)}
          ${when(s.save, () => html`<md-filled-button @click=${() => this.saveSheet(s)}>${s.saveLabel ?? '保存'}</md-filled-button>`)}
        </header>
        <div class="sheet-body">${s.render()}</div>
      </section>`)}

      <md-dialog .open=${!!this.dlg} @cancel=${(e) => { e.preventDefault(); this.finishDialog(this.dlg?.options ? null : false); }}>
        <div slot="headline">${this.dlg?.title}</div>
        <div slot="content" class="dlg-content">
          ${when(this.dlg?.body, () => html`<p>${this.dlg.body}</p>`)}
          ${when(this.dlg?.options, () => html`<div class="dlg-options">${this.dlg.options.map((o) => html`<button class=${o.danger ? 'danger' : ''} @click=${() => this.finishDialog(o.value)}>${icon(o.icon)}<span>${o.label}</span></button>`)}</div>`)}
        </div>
        <div slot="actions">
          ${(this.dlg?.actions ?? []).map((a) => a.primary
            ? html`<md-filled-button class=${a.danger ? 'danger' : ''} @click=${() => this.finishDialog(a.value)}>${a.label}</md-filled-button>`
            : html`<md-text-button @click=${() => this.finishDialog(a.value)}>${a.label}</md-text-button>`)}
        </div>
      </md-dialog>

      <div class="snackbar ${this.snack ? 'show' : ''}">${this.snack}</div>
      ${when(this.busy, () => html`<div class="busy"><md-circular-progress indeterminate></md-circular-progress><span>${this.busy}</span></div>`)}
    `;
  }
}

customElements.define('mt-app', App);
