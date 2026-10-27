import nodemailer from 'nodemailer';

const user = process.env.GMAIL_USER || '';
const pass = process.env.GMAIL_APP_PASSWORD || '';

export function isEmailConfigured(): boolean {
  return Boolean(user && pass);
}

if (process.env.NODE_ENV === 'production' && !isEmailConfigured()) {
  throw new Error('FATAL: GMAIL_USER and GMAIL_APP_PASSWORD environment variables are required in production.');
}

const transporter = isEmailConfigured()
  ? nodemailer.createTransport({
      host: 'smtp.gmail.com',
      port: 587,
      secure: false, // Port 587 uses STARTTLS
      auth: { user, pass },
      connectionTimeout: 10000,
    })
  : null;

async function deliverMail(opts: { from?: string; to: string; subject: string; text: string; html: string }) {
  if (!transporter) {
    console.log(`\n=======================================================`);
    console.log(`📧 [MOCK EMAIL DELIVERY - SMTP NOT CONFIGURED]`);
    console.log(`To:      ${opts.to}`);
    console.log(`Subject: ${opts.subject}`);
    console.log(`Text:    ${opts.text}`);
    console.log(`=======================================================\n`);
    return { messageId: `mock-dev-${Date.now()}` };
  }

  try {
    return await transporter.sendMail(opts);
  } catch (err) {
    console.error(`[AUTH SMTP ERROR] Failed to deliver mail:`, err);
    throw err;
  }
}

export async function sendOtpEmail({
  to,
  code,
  type = 'signup',
}: {
  to: string;
  code: string;
  type?: 'signup' | 'login' | 'verify' | 'reset';
}) {
  const title =
    type === 'signup'
      ? 'Verify your Spectra Labs account'
      : type === 'reset'
        ? 'Reset your Spectra Labs password'
        : 'Spectra Labs security verification';
  const heading = type === 'signup' ? 'Complete your registration' : type === 'reset' ? 'Reset your password' : 'Two-Factor Authentication';

  const html = `
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>${title}</title>
</head>
<body style="margin:0;padding:0;background-color:#040711;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#F1F5F9;">
  <table width="100%" cellpadding="0" cellspacing="0" style="background-color:#040711;padding:40px 20px;">
    <tr>
      <td align="center">
        <table width="100%" cellpadding="0" cellspacing="0" style="max-width:480px;background-color:#0B0F19;border:1px solid rgba(0,242,254,0.25);border-radius:16px;padding:36px 32px;box-shadow:0 0 40px rgba(0,242,254,0.15);">
          <!-- Logo & Brand Header -->
          <tr>
            <td align="center" style="padding-bottom:24px;">
              <div style="display:inline-block;padding:8px 16px;background:rgba(0,242,254,0.08);border:1px solid rgba(0,242,254,0.3);border-radius:999px;">
                <span style="font-family:'JetBrains Mono',monospace;font-size:12px;font-weight:700;letter-spacing:0.2em;color:#00F2FE;text-transform:uppercase;">
                  Spectra Labs · Security
                </span>
              </div>
            </td>
          </tr>

          <!-- Main Heading -->
          <tr>
            <td align="center" style="padding-bottom:12px;">
              <h1 style="margin:0;font-size:22px;font-weight:700;color:#FFFFFF;letter-spacing:-0.02em;">
                ${heading}
              </h1>
            </td>
          </tr>

          <!-- Description -->
          <tr>
            <td align="center" style="padding-bottom:28px;">
              <p style="margin:0;font-size:14px;line-height:22px;color:#94A3B8;">
                Use the 6-digit one-time code below to verify your identity and access the institutional quant platform.
              </p>
            </td>
          </tr>

          <!-- OTP Code Box -->
          <tr>
            <td align="center" style="padding-bottom:28px;">
              <div style="background-color:#040711;border:1px solid rgba(0,242,254,0.5);border-radius:12px;padding:18px 24px;box-shadow:0 0 24px rgba(0,242,254,0.2);">
                <span style="font-family:'JetBrains Mono',monospace,Courier;font-size:32px;font-weight:800;letter-spacing:0.35em;color:#00F2FE;">
                  ${code}
                </span>
              </div>
            </td>
          </tr>

          <!-- Expiry Notice -->
          <tr>
            <td align="center" style="padding-bottom:24px;">
              <p style="margin:0;font-size:12px;color:#64748B;">
                ⏱️ This code will expire in <strong style="color:#CBD5E1;">5 minutes</strong>.
              </p>
            </td>
          </tr>

          <!-- Divider -->
          <tr>
            <td style="border-top:1px solid rgba(255,255,255,0.08);padding-bottom:20px;"></td>
          </tr>

          <!-- Security Footer -->
          <tr>
            <td align="center">
              <p style="margin:0;font-size:11px;line-height:18px;color:#475569;">
                If you did not request this verification code, please ignore this email. No action is required.
              </p>
              <p style="margin:8px 0 0 0;font-size:10px;color:#334155;font-family:'JetBrains Mono',monospace;">
                Spectra Labs Inc. · Pure Rust Quantitative Engine
              </p>
            </td>
          </tr>
        </table>
      </td>
    </tr>
  </table>
</body>
</html>
  `;

  return deliverMail({
    from: `"Spectra Labs Security" <${user || 'security@spectralabs.dev'}>`,
    to,
    subject:
      type === 'reset'
        ? `[${code}] Your Spectra Labs password reset code`
        : type === 'login'
          ? `[${code}] Your Spectra Labs login code`
          : `[${code}] Your Spectra Labs Verification Code`,
    text:
      type === 'reset'
        ? `Your Spectra Labs password reset code is: ${code}. It expires in 5 minutes. If you didn't ask to reset your password, ignore this email; your password stays the same.`
        : `Your Spectra Labs 6-digit verification code is: ${code}. This code expires in 5 minutes.`,
    html,
  });
}

/**
 * Tell an account owner their sign-in was locked after repeated wrong passwords (M19 task 7).
 * Callers rate-limit this (see shouldSendLockoutEmail) and never let a send failure block a request.
 */
export async function sendLockoutEmail({ to, minutes, ip }: { to: string; minutes: number; ip: string }) {
  const where = ip && ip !== 'unknown' ? ` from IP address ${ip}` : '';
  const text =
    `There were 10 failed sign-in attempts on your Spectra Labs account${where}, so sign-in is locked for ${minutes} minutes.\n\n` +
    `If this was you, wait or reset your password to unlock it now.\n` +
    `If it wasn't you, someone may know or be guessing your email address: reset your password, and keep two-factor login on.`;
  const html = `
<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>Sign-in locked</title></head>
<body style="margin:0;padding:0;background-color:#040711;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#F1F5F9;">
  <table width="100%" cellpadding="0" cellspacing="0" style="background-color:#040711;padding:40px 20px;">
    <tr><td align="center">
      <table width="100%" cellpadding="0" cellspacing="0" style="max-width:480px;background-color:#0B0F19;border:1px solid rgba(251,191,36,0.35);border-radius:16px;padding:36px 32px;">
        <tr><td align="center" style="padding-bottom:20px;">
          <span style="font-family:'JetBrains Mono',monospace;font-size:12px;font-weight:700;letter-spacing:0.2em;color:#FBBF24;text-transform:uppercase;">Spectra Labs · Security alert</span>
        </td></tr>
        <tr><td align="center" style="padding-bottom:12px;">
          <h1 style="margin:0;font-size:22px;font-weight:700;color:#FFFFFF;">Sign-in temporarily locked</h1>
        </td></tr>
        <tr><td style="padding-bottom:20px;">
          <p style="margin:0;font-size:14px;line-height:22px;color:#94A3B8;">
            There were 10 failed sign-in attempts on your account${where.replace(/[<>&"]/g, '')}, so sign-in is locked for
            <strong style="color:#E2E8F0;">${minutes} minutes</strong>.
          </p>
        </td></tr>
        <tr><td style="padding-bottom:24px;">
          <p style="margin:0;font-size:14px;line-height:22px;color:#94A3B8;">
            <strong style="color:#E2E8F0;">If this was you</strong>, wait, or reset your password to unlock it now.<br>
            <strong style="color:#E2E8F0;">If it wasn't</strong>, someone may be guessing your password: reset it and keep two-factor login on.
          </p>
        </td></tr>
        <tr><td style="border-top:1px solid rgba(255,255,255,0.08);padding-top:16px;" align="center">
          <p style="margin:0;font-size:10px;color:#334155;font-family:'JetBrains Mono',monospace;">Spectra Labs Inc. · Pure Rust Quantitative Engine</p>
        </td></tr>
      </table>
    </td></tr>
  </table>
</body>
</html>`;
  return deliverMail({ from: `"Spectra Labs Security" <${user || 'security@spectralabs.dev'}>`, to, subject: 'Sign-in to your Spectra Labs account was locked', text, html });
}

const escapeHtml = (s: string) => s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] as string);

/**
 * "New sign-in to your account" (M19 task 11): a password sign-in from a browser this account
 * hasn't used before. Callers rate-limit it and never let a send failure block the sign-in.
 * APP_URL (optional, e.g. https://app.spectralabs.dev) adds a link to the Account page.
 */
export async function sendNewDeviceEmail({ to, device, ip, when }: { to: string; device: string; ip: string; when: Date }) {
  const whenText = when.toUTCString();
  const where = ip && ip !== 'unknown' ? ip : 'an unknown network';
  const account = process.env.APP_URL ? `${process.env.APP_URL.replace(/\/+$/, '')}/account` : '';
  const text =
    `Your Spectra Labs account was just signed in to from a device it hasn't used before.\n\n` +
    `Device: ${device}\nIP address: ${where}\nTime: ${whenText}\n\n` +
    `If this was you, there's nothing to do.\n` +
    `If it wasn't, reset your password now and sign out the other devices${account ? `: ${account}` : ' on your Account page'}. Keep two-step login on.`;
  const row = (k: string, v: string) =>
    `<tr><td style="padding:4px 12px 4px 0;font-size:13px;color:#64748B;">${k}</td><td style="padding:4px 0;font-size:13px;color:#E2E8F0;font-family:'JetBrains Mono',monospace;">${escapeHtml(v)}</td></tr>`;
  const html = `
<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>New sign-in</title></head>
<body style="margin:0;padding:0;background-color:#040711;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#F1F5F9;">
  <table width="100%" cellpadding="0" cellspacing="0" style="background-color:#040711;padding:40px 20px;">
    <tr><td align="center">
      <table width="100%" cellpadding="0" cellspacing="0" style="max-width:480px;background-color:#0B0F19;border:1px solid rgba(0,242,254,0.25);border-radius:16px;padding:36px 32px;">
        <tr><td align="center" style="padding-bottom:20px;">
          <span style="font-family:'JetBrains Mono',monospace;font-size:12px;font-weight:700;letter-spacing:0.2em;color:#00F2FE;text-transform:uppercase;">Spectra Labs · Security</span>
        </td></tr>
        <tr><td align="center" style="padding-bottom:12px;">
          <h1 style="margin:0;font-size:22px;font-weight:700;color:#FFFFFF;">New sign-in to your account</h1>
        </td></tr>
        <tr><td style="padding-bottom:16px;">
          <p style="margin:0;font-size:14px;line-height:22px;color:#94A3B8;">Your account was just signed in to from a device it hasn&#39;t used before.</p>
        </td></tr>
        <tr><td style="padding-bottom:20px;"><table cellpadding="0" cellspacing="0">${row('Device', device)}${row('IP address', where)}${row('Time', whenText)}</table></td></tr>
        <tr><td style="padding-bottom:24px;">
          <p style="margin:0;font-size:14px;line-height:22px;color:#94A3B8;">
            <strong style="color:#E2E8F0;">If this was you</strong>, there&#39;s nothing to do.<br>
            <strong style="color:#E2E8F0;">If it wasn&#39;t</strong>, reset your password now and sign out the other devices${account ? ` on your <a href="${escapeHtml(account)}" style="color:#00F2FE;">Account page</a>` : ' on your Account page'}.
          </p>
        </td></tr>
        <tr><td style="border-top:1px solid rgba(255,255,255,0.08);padding-top:16px;" align="center">
          <p style="margin:0;font-size:10px;color:#334155;font-family:'JetBrains Mono',monospace;">Spectra Labs Inc. · Pure Rust Quantitative Engine</p>
        </td></tr>
      </table>
    </td></tr>
  </table>
</body>
</html>`;
  return deliverMail({ from: `"Spectra Labs Security" <${user || 'security@spectralabs.dev'}>`, to, subject: 'New sign-in to your Spectra Labs account', text, html });
}

/** Workspace invitation (M19 task 12). The link is single-use and valid for 7 days. */
export async function sendInviteEmail({ to, inviter, workspace, role, link }: { to: string; inviter: string; workspace: string; role: string; link: string }) {
  const text =
    `${inviter} invited you to join the "${workspace}" workspace on Spectra Labs as ${role === 'editor' ? 'an editor' : 'a viewer'}.\n\n` +
    `Accept: ${link}\n\n` +
    `The link works once and expires in 7 days. Sign in (or create an account) with this email address to accept. ` +
    `If you weren't expecting this, ignore it: nothing happens unless you accept.`;
  const html = `
<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>Workspace invitation</title></head>
<body style="margin:0;padding:0;background-color:#040711;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#F1F5F9;">
  <table width="100%" cellpadding="0" cellspacing="0" style="background-color:#040711;padding:40px 20px;">
    <tr><td align="center">
      <table width="100%" cellpadding="0" cellspacing="0" style="max-width:480px;background-color:#0B0F19;border:1px solid rgba(0,242,254,0.25);border-radius:16px;padding:36px 32px;">
        <tr><td align="center" style="padding-bottom:20px;">
          <span style="font-family:'JetBrains Mono',monospace;font-size:12px;font-weight:700;letter-spacing:0.2em;color:#00F2FE;text-transform:uppercase;">Spectra Labs</span>
        </td></tr>
        <tr><td align="center" style="padding-bottom:12px;">
          <h1 style="margin:0;font-size:22px;font-weight:700;color:#FFFFFF;">Join ${escapeHtml(workspace)}</h1>
        </td></tr>
        <tr><td align="center" style="padding-bottom:24px;">
          <p style="margin:0;font-size:14px;line-height:22px;color:#94A3B8;">${escapeHtml(inviter)} invited you to the <strong style="color:#E2E8F0;">${escapeHtml(workspace)}</strong> workspace as ${role === 'editor' ? 'an editor' : 'a viewer'}.</p>
        </td></tr>
        <tr><td align="center" style="padding-bottom:24px;">
          <a href="${escapeHtml(link)}" style="display:inline-block;padding:12px 22px;border-radius:10px;background:#00F2FE;color:#03131A;font-weight:700;font-size:14px;text-decoration:none;">Accept invitation</a>
        </td></tr>
        <tr><td align="center" style="padding-bottom:20px;">
          <p style="margin:0;font-size:12px;line-height:18px;color:#64748B;">The link works once and expires in 7 days. Sign in with this email address to accept. Not expecting this? Ignore it.</p>
        </td></tr>
        <tr><td style="border-top:1px solid rgba(255,255,255,0.08);padding-top:16px;" align="center">
          <p style="margin:0;font-size:10px;color:#334155;font-family:'JetBrains Mono',monospace;">Spectra Labs Inc. · Pure Rust Quantitative Engine</p>
        </td></tr>
      </table>
    </td></tr>
  </table>
</body>
</html>`;
  return deliverMail({ from: `"Spectra Labs" <${user || 'team@spectralabs.dev'}>`, to, subject: `${inviter} invited you to ${workspace} on Spectra Labs`, text, html });
}

/**
 * Site-wide security alert to the team (M19 task 18; see lib/server/spikes.ts): a spike of failed
 * sign-ins or emailed codes. Plain facts only; the audit log has the detail.
 */
export async function sendSecurityAlertEmail({ to, summary, alert }: { to: string[]; summary: string; alert: { kind: string; topIps?: { ip: string; count: number }[] } }) {
  const what = alert.kind === 'failed_login' ? 'failed sign-ins' : 'code email requests';
  const advice =
    alert.kind === 'failed_login'
      ? 'Check Admin → Activity for auth.login.failure. Many accounts from a few addresses suggests credential stuffing: consider blocking those addresses at the proxy.'
      : 'Check whether sign-up, reset or login codes are being requested for addresses that never asked: the code emails may be used to spam people or to burn the sending quota.';
  const rows = (alert.topIps ?? [])
    .map((t) => `<tr><td style="padding:2px 12px 2px 0;font-family:'JetBrains Mono',monospace;font-size:12px;color:#E2E8F0;">${escapeHtml(t.ip)}</td><td style="font-size:12px;color:#94A3B8;">${t.count}</td></tr>`)
    .join('');
  const html = `
<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>Security alert</title></head>
<body style="margin:0;padding:0;background-color:#040711;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#F1F5F9;">
  <table width="100%" cellpadding="0" cellspacing="0" style="background-color:#040711;padding:40px 20px;">
    <tr><td align="center">
      <table width="100%" cellpadding="0" cellspacing="0" style="max-width:520px;background-color:#0B0F19;border:1px solid rgba(244,63,94,0.4);border-radius:16px;padding:32px;">
        <tr><td style="padding-bottom:16px;">
          <span style="font-family:'JetBrains Mono',monospace;font-size:12px;font-weight:700;letter-spacing:0.2em;color:#FB7185;text-transform:uppercase;">Spectra Labs · Ops alert</span>
        </td></tr>
        <tr><td style="padding-bottom:12px;"><h1 style="margin:0;font-size:20px;color:#FFFFFF;">Spike in ${what}</h1></td></tr>
        <tr><td style="padding-bottom:16px;"><p style="margin:0;font-size:14px;line-height:22px;color:#94A3B8;">${escapeHtml(summary)}</p></td></tr>
        ${rows ? `<tr><td style="padding-bottom:16px;"><table cellpadding="0" cellspacing="0">${rows}</table></td></tr>` : ''}
        <tr><td><p style="margin:0;font-size:13px;line-height:20px;color:#94A3B8;">${escapeHtml(advice)} You won't get another alert of this kind for an hour.</p></td></tr>
      </table>
    </td></tr>
  </table>
</body>
</html>`;
  return deliverMail({
    from: `"Spectra Labs Security" <${user || 'security@spectralabs.dev'}>`,
    to: to.join(', '),
    subject: `[Spectra Labs] Security alert: spike in ${what}`,
    text: `${summary}\n\n${advice}\nYou won't get another alert of this kind for an hour.`,
    html,
  });
}
