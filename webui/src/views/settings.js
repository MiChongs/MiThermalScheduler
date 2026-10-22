import { html } from 'lit';
import { icon } from '../ui.js';

function row(ic, title, sub, trailing, onClick) {
  return html`<div class="list-row ${onClick ? 'clickable' : ''}" @click=${onClick}>
    <div class="avatar small">${icon(ic)}</div>
    <div class="grow"><div class="row-title">${title}</div><div class="row-sub">${sub}</div></div>
    ${trailing ?? icon('chevron_right', 'dim')}
  </div>`;
}

export function renderSettings(app) {
  const cfg = app.cfg;
  const dev = app.info.device;
  const running = app.status?.daemon.running;
  return html`
    <div class="section-head first"><h2>运行</h2></div>
    <div class="card-outlined">
      <label class="list-row switch-row">
        <div class="avatar small">${icon('lock')}</div>
        <div class="grow"><div class="row-title">锁定覆盖文件</div><div class="row-sub">chattr +i，阻止云控下发的温控配置覆盖本模块</div></div>
        <md-switch .selected=${cfg.immutable} @change=${(e) => app.edit((c) => { c.immutable = e.target.selected; })}></md-switch>
      </label>
      <div class="list-row">
        <div class="avatar small">${icon('schedule')}</div>
        <div class="grow"><div class="row-title">检测间隔</div><div class="row-sub">自动化与过热保护的检测频率，越短响应越快</div></div>
        <select class="native-select" .value=${String(cfg.interval)} @change=${(e) => app.edit((c) => { c.interval = Number(e.target.value); })}>
          ${[1000, 2000, 3000, 5000].map((v) => html`<option value=${v} ?selected=${cfg.interval === v}>${v / 1000} 秒</option>`)}
        </select>
      </div>
      ${row('restart_alt', running ? '重启守护进程' : '启动守护进程', running ? `运行中 · pid ${app.status.daemon.state.pid}` : '未运行', null, () => app.restartDaemon())}
      ${row('play_arrow', '立即重新应用', '重新生成并写入当前方案', null, () => app.reapply())}
    </div>

    <div class="section-head"><h2>配置</h2></div>
    <div class="card-outlined">
      ${row('upload', '导出配置', '复制全部方案与规则为 JSON', null, () => app.exportConfig())}
      ${row('download', '导入配置', '粘贴 JSON 覆盖当前配置', null, () => app.importConfig())}
      ${row('code', '查看当前加载的配置', 'mi_thermald 正在使用的解密配置', null, () => app.viewLoaded())}
      ${row('undo', '恢复官方温控', '关闭总开关并删除所有覆盖文件', null, () => app.resetAll())}
    </div>

    <div class="section-head"><h2>关于</h2></div>
    <div class="card-outlined pad about">
      <div class="about-head">
        <div class="avatar big">${icon('device_thermostat')}</div>
        <div>
          <div class="about-name">小米温控调度</div>
          <div class="row-sub">版本 ${app.info.version} · Rust 后端 + Material 3 WebUI</div>
        </div>
      </div>
      <div class="kv3">
        <div><span>设备</span><b>${dev.model}</b></div>
        <div><span>代号</span><b>${dev.codename}</b></div>
        <div><span>平台</span><b>${dev.soc}</b></div>
        <div><span>系统</span><b>${dev.os} · Android ${dev.android}</b></div>
        <div><span>版本号</span><b class="mono">${dev.build}</b></div>
        <div><span>配置密钥</span><b>${app.info.cipherKey === 0 ? '旧版 · thermalopenssl.h' : '新版'}</b></div>
        <div><span>场景配置</span><b>${app.scenes ? `${app.scenes.list.length} 个` : '—'}</b></div>
      </div>
      <p class="hint flat">本模块通过 mi_thermald 官方支持的覆盖目录 /data/vendor/thermal/config 生效，不修改系统分区。卸载或停用模块后会自动恢复官方温控。</p>
    </div>
  `;
}
