// KernelSU 桥接与 thermalctl 命令封装
export const MOD = '/data/adb/modules/mi_thermal_m3';
export const BIN = `${MOD}/bin/thermalctl`;
const TMP = '/data/adb/mi_thermal_m3/.webui';

let seq = 0;

export const hasKsu = () => typeof window.ksu !== 'undefined';

export function exec(cmd) {
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

/** 运行 thermalctl 子命令并解析 JSON 输出 */
export async function ctl(args) {
  const r = await exec(`${BIN} ${args}`);
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
