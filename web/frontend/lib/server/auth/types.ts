/** Storage for accounts and emailed codes. Postgres in deployments, a JSON file in local dev/tests. */

export type Role = 'admin' | 'user';

export type UserRecord = {
  /** UUID (UUIDv7 from Postgres). */
  id: string;
  email: string;
  name?: string;
  passwordHash: string;
  role: Role;
  createdAt: number;
  /** ms epoch of the last password reset; sessions issued before it are rejected. */
  passwordChangedAt?: number;
  /** Ask for an emailed code at every login (in addition to the password). */
  twoFactor?: boolean;
  /** The user's default workspace (Postgres only). */
  workspaceId?: string;
};

export type NewUser = Pick<UserRecord, 'email' | 'name' | 'passwordHash' | 'role' | 'twoFactor'>;

export type CodeKind = 'signup' | 'reset' | 'login';

export type PendingCode = {
  /** SHA-256 of the 6-digit code; the code itself is never stored. */
  codeHash: string;
  passwordHash: string;
  name: string;
  /** Sign-up only: the 2FA choice made on the form, applied when the account is created. */
  twoFactor?: boolean;
  expiresAt: number;
  attempts: number;
};

export type CodeCheck = 'ok' | 'missing' | 'locked' | 'wrong';

export const OTP_MAX_ATTEMPTS = 5;

export interface AuthBackend {
  findUser(email: string): Promise<UserRecord | undefined>;
  /** Creates the account (and, in Postgres, its default workspace). Returns the existing record if the email is taken. */
  insertUser(user: NewUser): Promise<UserRecord>;
  updateUser(
    email: string,
    patch: { passwordHash?: string; passwordChangedAt?: number; twoFactor?: boolean },
  ): Promise<UserRecord | undefined>;
  /** Every account, newest first. */
  listUsers(): Promise<UserRecord[]>;

  putCode(kind: CodeKind, email: string, code: PendingCode): Promise<void>;
  /** The live code, if any (expired codes are removed and reported as missing). */
  peekCode(kind: CodeKind, email: string): Promise<PendingCode | undefined>;
  /**
   * Checks a code atomically: single-use on success, counts wrong attempts, and locks
   * (deletes) the code after OTP_MAX_ATTEMPTS misses.
   */
  consumeCode(kind: CodeKind, email: string, codeHash: string): Promise<{ result: CodeCheck; entry?: PendingCode }>;
}
