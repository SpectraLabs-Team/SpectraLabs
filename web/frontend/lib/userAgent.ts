/**
 * Turn a User-Agent string into something a person recognises ("Chrome on macOS"). Shared by the
 * server (new-device emails) and the Account page. Best effort: browsers lie, and that's fine for
 * a label. Order matters: Edge and Opera also say "Chrome", Chrome also says "Safari".
 */
export type DeviceInfo = { browser: string; os: string; kind: 'desktop' | 'mobile' | 'tablet'; label: string };

const BROWSERS: [RegExp, string][] = [
  [/\bEdg(?:e|A|iOS)?\//, 'Edge'],
  [/\b(?:OPR|Opera)\//, 'Opera'],
  [/\bSamsungBrowser\//, 'Samsung Internet'],
  [/\bFirefox\/|\bFxiOS\//, 'Firefox'],
  [/\bCriOS\/|\bChrome\//, 'Chrome'],
  [/\bVersion\/[\d.]+.*\bSafari\//, 'Safari'],
  [/\bcurl\//, 'curl'],
  [/\bnode-fetch\b|\bundici\b|\bNode\.js\b/, 'Node.js'],
];

const SYSTEMS: [RegExp, string][] = [
  [/\biPhone\b/, 'iPhone'],
  [/\biPad\b/, 'iPad'],
  [/\bAndroid\b/, 'Android'],
  [/\bCrOS\b/, 'ChromeOS'],
  [/\bWindows\b/, 'Windows'],
  [/\bMac OS X\b|\bMacintosh\b/, 'macOS'],
  [/\bLinux\b/, 'Linux'],
];

export function describeUserAgent(ua: string | null | undefined): DeviceInfo {
  const s = ua ?? '';
  const browser = BROWSERS.find(([re]) => re.test(s))?.[1] ?? 'Unknown browser';
  const os = SYSTEMS.find(([re]) => re.test(s))?.[1] ?? 'unknown system';
  const kind: DeviceInfo['kind'] = /\biPad\b|\bTablet\b/.test(s) || (/\bAndroid\b/.test(s) && !/\bMobile\b/.test(s)) ? 'tablet' : /\bMobi|\biPhone\b/.test(s) ? 'mobile' : 'desktop';
  return { browser, os, kind, label: s ? `${browser} on ${os}` : 'Unknown device' };
}
