// Shapes of the admin console's JSON (lib/server/adminAnalytics.ts produces them). Type-only,
// so client components can import them without pulling in server code.

export type DateRange = { from: string; to: string; days: number };

export type AdminOverview = {
  generatedAt: number;
  users: { total: number; admins: number; disabled: number; registeredLast30Days: number };
  activity: { activeToday: number; activeLast7Days: number; activeLast30Days: number; retentionDays: number };
  simulations: { runsLast24h: number; runsFailedLast24h: number; jobsFailedLast24h: number } | null;
  paperOrders: { placedLast24h: number; rejectedLast24h: number } | null;
  security: { failedLoginsLast24h: number; alertsLast24h: number };
  workers: { id: string; hostname: string; version: string; kinds: string[]; concurrency: number; online: boolean; heartbeatAt: number; runningJobs: number }[] | null;
  engine: { runningJobs: number; queuedJobs: number } | null;
};

export type AnalyticsDay = {
  day: string;
  activeMinutes: number;
  activeUsers: number;
  signups: number;
  logins: number;
  loginFailures: number;
  runs: number;
  runsFailed: number;
  paperOrders: number;
  paperRejected: number;
  jobsFailed: number;
};

export type AdminAnalytics = {
  range: DateRange;
  totals: Omit<AnalyticsDay, 'day'>;
  series: AnalyticsDay[];
};

export type DirectoryRow = {
  id: string;
  email: string;
  name: string;
  role: 'admin' | 'user';
  status: 'active' | 'disabled';
  createdAt: number;
  activeMinutes30d: number;
  lastSeenAt: number | null;
};
