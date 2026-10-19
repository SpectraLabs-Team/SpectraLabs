import QRCode from 'qrcode';

/**
 * Renders text (e.g. an otpauth:// URI) as an SVG string.
 *
 * NOTE: The input string frequently contains shared secrets (e.g. TOTP secrets),
 * so never log the input or include it in error messages.
 */
export async function toQrSvg(text: string): Promise<string> {
  if (!text || typeof text !== 'string' || text.length === 0) {
    throw new Error('Text must not be empty');
  }
  if (text.length > 1024) {
    throw new Error('Text exceeds maximum length of 1024 characters');
  }

  return QRCode.toString(text, {
    type: 'svg',
    errorCorrectionLevel: 'M',
    margin: 1,
  });
}
