"use client";

import { useState } from "react";

interface DeleteDialogProps {
  name: string;
  onConfirm: () => Promise<void>;
  onCancel: () => void;
}

export function DeleteDialog({ name, onConfirm, onCancel }: DeleteDialogProps) {
  const [deleting, setDeleting] = useState(false);

  async function handleDelete() {
    setDeleting(true);
    await onConfirm();
  }

  return (
    <div className="fixed inset-0 bg-black/40 flex items-center justify-center z-50">
      <div className="bg-white rounded-lg p-6 max-w-md w-full mx-4 shadow-xl">
        <h3 className="text-lg font-semibold text-gray-900">Delete Account</h3>
        <p className="mt-2 text-gray-600">
          Are you sure you want to delete <strong>{name}</strong>? This will
          also delete all holdings. This action cannot be undone.
        </p>
        <div className="mt-4 flex gap-3 justify-end">
          <button
            onClick={onCancel}
            disabled={deleting}
            className="px-4 py-2 border border-gray-300 rounded-md hover:bg-gray-50 transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleDelete}
            disabled={deleting}
            className="px-4 py-2 bg-red-600 text-white rounded-md hover:bg-red-700 disabled:opacity-50 transition-colors"
          >
            {deleting ? "Deleting..." : "Delete"}
          </button>
        </div>
      </div>
    </div>
  );
}
