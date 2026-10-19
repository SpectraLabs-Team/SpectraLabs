import type { Metadata } from 'next';
import { InviteAccept } from '@/components/team/InviteAccept';
import { Footer } from '@/components/landing/Footer';
import { Header } from '@/components/landing/Header';

export const metadata: Metadata = { title: 'Workspace invitation', robots: { index: false } };

/** Landing page for an emailed workspace invite (M19 task 12). Public; accepting needs sign-in. */
export default function InvitePage({ params }: { params: { token: string } }) {
  return (
    <>
      <Header />
      <main className="mx-auto max-w-[560px] px-5 py-28 sm:px-8">
        <InviteAccept token={params.token} />
      </main>
      <Footer />
    </>
  );
}
