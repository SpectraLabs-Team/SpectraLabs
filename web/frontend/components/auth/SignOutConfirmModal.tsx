'use client';

import { AnimatePresence, motion } from 'framer-motion';
import { LogOut, X } from 'lucide-react';
import { useEffect } from 'react';

interface SignOutConfirmModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => void;
}

export function SignOutConfirmModal({ isOpen, onClose, onConfirm }: SignOutConfirmModalProps) {
  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, onClose]);

  return (
    <AnimatePresence>
      {isOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={onClose}
            className="fixed inset-0 bg-black/60 backdrop-blur-sm"
          />
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-labelledby="signout-modal-title"
            initial={{ opacity: 0, scale: 0.95, y: 10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: 10 }}
            transition={{ type: 'spring', damping: 25, stiffness: 350 }}
            className="relative w-full max-w-sm rounded-2xl border border-ink/10 bg-panel p-6 shadow-2xl z-10"
          >
            <button
              type="button"
              onClick={onClose}
              className="absolute right-4 top-4 rounded-lg p-1 text-ink/40 transition hover:bg-ink/[0.06] hover:text-ink"
              aria-label="Close"
            >
              <X className="h-4 w-4" />
            </button>
            <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-loss/10 text-loss">
              <LogOut className="h-6 w-6" />
            </div>
            <h3 id="signout-modal-title" className="mt-4 text-lg font-bold text-ink">
              Sign out of Spectra Labs?
            </h3>
            <p className="mt-2 text-sm text-ink/60">
              Are you sure you want to sign out? You will need to log back in to access your workspaces and trading features.
            </p>
            <div className="mt-6 flex items-center justify-end gap-3">
              <button
                type="button"
                onClick={onClose}
                className="rounded-xl border border-ink/15 px-4 py-2 text-sm font-semibold text-ink/70 transition hover:bg-ink/[0.04] hover:text-ink"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => {
                  onClose();
                  onConfirm();
                }}
                className="rounded-xl bg-loss px-4 py-2 text-sm font-bold text-white shadow-sm transition hover:bg-loss/90"
              >
                Sign Out
              </button>
            </div>
          </motion.div>
        </div>
      )}
    </AnimatePresence>
  );
}
