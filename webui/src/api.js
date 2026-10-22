// KernelSU 桥接与 thermalctl 命令封装
export const MOD = '/data/adb/modules/mi_thermal_m3';
export const BIN = `${MOD}/bin/thermalctl`;
const TMP = '/data/adb/mi_thermal_m3/.webui';

let seq = 0;

export const hasKsu = () => typeof window.ksu !== 'undefined';

/** ksu.exec 是同步接口，执行期间会阻塞页面；仅在 spawn 不可用时使用 */
function execBlocking(cmd) {
  return new Promise((resolve) => {
    const cb = `__ksu_cb_${Date.now()}_${seq++}`;
    window[cb] = (errno, stdout, stderr) => {
      delete window[cb];
      resolve({ errno, stdout: stdout ?? '', stderr: stderr ?? '' });
    };
    try {
      window.ksu.exec(cmd, '{}', cb);
    } catch (e) {
      delete window[cb];
      resolve({ errno: -1, stdout: '', stderr: String(e) });
    }
  });
}

/** 通过异步的 ksu.spawn 执行 shell 命令，不阻塞页面；输出按行回传 */
export function exec(cmd) {
  if (typeof window.ksu?.spawn !== 'function') return execBlocking(cmd);
  return new Promise((resolve) => {
    const name = `__ksu_spawn_${Date.now()}_${seq++}`;
    const out = [];
    const err = [];
    const finish = (r) => {
      delete window[name];
      resolve(r);
    };
    window[name] = {
      stdout: { emit: (ev, data) => { if (ev === 'data') out.push(data); } },
      stderr: { emit: (ev, data) => { if (ev === 'data') err.push(data); } },
      stdin: { emit: () => {} },
      emit: (ev, arg) => {
        if (ev === 'exit') finish({ errno: Number(arg) || 0, stdout: out.join('\n'), stderr: err.join('\n') });
        else if (ev === 'error') finish({ errno: -1, stdout: out.join('\n'), stderr: String(arg) });
      },
    };
    try {
      // KernelSU 会把 command 与 args 以空格拼接后交给 shell 执行，因此整条命令放在 command 中
      window.ksu.spawn(cmd, '[]', '{}', name);
    } catch {
      delete window[name];
      execBlocking(cmd).then(resolve);
    }
  });
}

/** 运行 thermalctl 子命令并解析 JSON 输出 */
export async function ctl(args, prefix = '') {
  const r = await exec(`${prefix}${BIN} ${args}`);
  let data;
  try {
    data = JSON.parse(r.stdout);
  } catch {
    throw new Error(r.stderr.trim() || r.stdout.trim() || `thermalctl 退出码 ${r.errno}`);
  }
  if (data && data.ok === false) throw new Error(data.error);
  return data;
}

export async function text(cmd) {
  const r = await exec(cmd);
  return r.stdout;
}

function toB64(str) {
  const bytes = new TextEncoder().encode(str);
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

/** 把字符串写入设备上的临时文件，返回路径 */
export async function writeTemp(name, content) {
  const path = `${TMP}/${name}`;
  const b64 = toB64(content);
  const CHUNK = 60000;
  await exec(`mkdir -p ${TMP} && : > ${path}`);
  for (let i = 0; i < b64.length; i += CHUNK) {
    const r = await exec(`echo '${b64.slice(i, i + CHUNK)}' | base64 -d >> ${path}`);
    if (r.errno !== 0) throw new Error(r.stderr || '写入临时文件失败');
  }
  return path;
}

export async function saveConfig(cfg) {
  const b64 = toB64(JSON.stringify(cfg));
  // 常见大小的配置一次调用完成写入与保存
  if (b64.length < 100000) {
    const path = `${TMP}/config.json`;
    return ctl(`save ${path}`, `mkdir -p ${TMP} && echo '${b64}' | base64 -d > ${path} && `);
  }
  const path = await writeTemp('config.json', JSON.stringify(cfg));
  return ctl(`save ${path}`);
}

export async function previewProfile(profile, scene) {
  const path = await writeTemp('preview.json', JSON.stringify(profile));
  return ctl(`preview --file ${path}${scene ? ` --scene ${scene}` : ''}`);
}

/** 用户应用列表（优先使用 KernelSU 接口获取名称） */
export async function listApps() {
  let pkgs = [];
  try {
    pkgs = JSON.parse(window.ksu.listPackages('user'));
  } catch {
    const out = await text('pm list packages -3');
    pkgs = out.split('\n').map((l) => l.replace('package:', '').trim()).filter(Boolean);
  }
  let infos = [];
  try {
    infos = JSON.parse(window.ksu.getPackagesInfo(JSON.stringify(pkgs)));
  } catch {
    infos = [];
  }
  const label = new Map(infos.map((i) => [i.packageName, i.appLabel]));
  return pkgs.map((p) => ({ pkg: p, label: label.get(p) || p })).sort((a, b) => a.label.localeCompare(b.label, 'zh'));
}

export function toast(msg) {
  try {
    window.ksu.toast(msg);
  } catch {
    console.log(msg);
  }
}
