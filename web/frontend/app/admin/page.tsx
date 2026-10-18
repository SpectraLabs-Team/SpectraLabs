import type { Metadata } from 'next';
import { RequireAdmin } from '@/components/auth/Gates';
import { AdminDashboard } from '@/components/admin/AdminDashboard';
import { Header } from '@/components/landing/Header';

export const metadata: Metadata = { title: 'Admin', robots: { index: false } };

export default function AdminPage() {
  return (
    <RequireAdmin>
      <Header />
      <main className="mx-auto max-w-[1320px] px-5 py-24 sm:px-8">
        <AdminDashboard />
      </main>
    </RequireAdmin>
  );
}
