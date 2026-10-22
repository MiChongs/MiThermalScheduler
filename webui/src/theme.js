// Material 3 配色：优先使用 KernelSU 管理器提供的莫奈取色 (/internal/colors.css)，
// 不可用时由种子色生成 tonal spot 方案
import { Hct, SchemeTonalSpot, MaterialDynamicColors, hexFromArgb, argbFromHex } from '@material/material-color-utilities';

const ROLES = [
  'primary', 'onPrimary', 'primaryContainer', 'onPrimaryContainer', 'inversePrimary',
  'secondary', 'onSecondary', 'secondaryContainer', 'onSecondaryContainer',
  'tertiary', 'onTertiary', 'tertiaryContainer', 'onTertiaryContainer',
  'error', 'onError', 'errorContainer', 'onErrorContainer',
  'background', 'onBackground', 'surface', 'onSurface', 'surfaceVariant', 'onSurfaceVariant',
  'surfaceDim', 'surfaceBright', 'surfaceContainerLowest', 'surfaceContainerLow', 'surfaceContainer',
  'surfaceContainerHigh', 'surfaceContainerHighest', 'surfaceTint',
  'inverseSurface', 'inverseOnSurface', 'outline', 'outlineVariant', 'shadow', 'scrim',
];

const kebab = (s) => s.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
const norm = (s) => s.toLowerCase().replace(/[^a-z]/g, '');

const SEED = '#D9653B';
const dark = () => matchMedia('(prefers-color-scheme: dark)').matches;

function applyTokens(colors) {
  const root = document.documentElement.style;
  for (const role of ROLES) {
    if (colors[role]) root.setProperty(`--md-sys-color-${kebab(role)}`, colors[role]);
  }
}

function seedColors(isDark) {
  const scheme = new SchemeTonalSpot(Hct.fromInt(argbFromHex(SEED)), isDark, 0);
  const out = {};
  for (const role of ROLES) out[role] = hexFromArgb(MaterialDynamicColors[role].getArgb(scheme));
  return out;
}

async function managerColors() {
  try {
    const res = await fetch('/internal/colors.css', { cache: 'no-store' });
    if (!res.ok) return null;
    const css = await res.text();
    const found = {};
    for (const m of css.matchAll(/--([\w-]+)\s*:\s*([^;]+);/g)) found[norm(m[1])] = m[2].trim();
    const out = {};
    for (const role of ROLES) if (found[norm(role)]) out[role] = found[norm(role)];
    return out.primary && out.surface ? out : null;
  } catch {
    return null;
  }
}

export async function initTheme() {
  const apply = async () => {
    const isDark = dark();
    document.documentElement.classList.toggle('dark', isDark);
    applyTokens(seedColors(isDark));
    const dyn = await managerColors();
    if (dyn) applyTokens(dyn);
  };
  await apply();
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', apply);
}
