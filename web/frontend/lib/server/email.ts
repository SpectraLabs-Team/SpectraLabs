import nodemailer from 'nodemailer';

const user = process.env.GMAIL_USER || 'i.practice.java@gmail.com';
const pass = process.env.GMAIL_APP_PASSWORD || '';

const transporter = nodemailer.createTransport({
  host: 'smtp.gmail.com',
  port: 587,
  secure: false, // Port 587 uses STARTTLS
  auth: { user, pass },
  connectionTimeout: 10000,
});

export async function sendOtpEmail({
  to,
  code,
  type = 'signup',
}: {
  to: string;
  code: string;
  type?: 'signup' | 'login' | 'verify';
}) {
  const title =
    type === 'signup'
      ? 'Verify your Spectra Labs account'
      : 'Spectra Labs security verification';

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
                ${type === 'signup' ? 'Complete your registration' : 'Two-Factor Authentication'}
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

  const info = await transporter.sendMail({
    from: `"Spectra Labs Security" <${user}>`,
    to,
    subject: `[${code}] Your Spectra Labs Verification Code`,
    text: `Your Spectra Labs 6-digit verification code is: ${code}. This code expires in 5 minutes.`,
    html,
  });

  return info;
}
