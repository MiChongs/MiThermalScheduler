import { build } from 'esbuild';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';

// 从 Material Symbols 中抽取用到的图标路径，生成 src/icons.gen.js
const ICONS = `dashboard tune automation history settings thermostat device_thermostat speed ac_unit eco balance
sports_esports gamepad local_fire_department verified bolt add edit delete content_copy check check_circle arrow_back
close memory developer_board deployed_code neurology battery_charging_full battery_full battery_alert shield apps
mobile_off play_arrow info upload download restart_alt lock code more_vert search rule star arrow_upward
arrow_downward warning visibility layers science schedule power_settings_new light_mode cell_tower more_horiz
description save undo sensors rocket_launch nightlight favorite help radio_button_checked radio_button_unchecked
chevron_right flash_on bug_report terminal swap_vert`.split(/\s+/);
const dir = 'node_modules/@material-symbols/svg-400/rounded';
const out = {};
for (const name of ICONS) {
  const file = [`${dir}/${name}-fill.svg`, `${dir}/${name}.svg`].find(existsSync);
  if (!file) {
    console.warn('icon missing:', name);
    continue;
  }
  out[name] = readFileSync(file, 'utf8').match(/ d="([^"]+)"/)[1];
}
writeFileSync('src/icons.gen.js', `export const ICONS = ${JSON.stringify(out)};\n`);

const dest = '../module/webroot';
rmSync(dest, { recursive: true, force: true });
mkdirSync(dest, { recursive: true });
await build({
  entryPoints: ['src/main.js'],
  bundle: true,
  minify: true,
  format: 'esm',
  target: ['chrome100'],
  outfile: `${dest}/main.js`,
  legalComments: 'none',
});
copyFileSync('src/index.html', `${dest}/index.html`);
copyFileSync('src/style.css', `${dest}/style.css`);
console.log('built', dest);
