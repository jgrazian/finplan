"use client";

import { useCallback, useEffect, useState } from "react";
import type { Account, AssetMapping, ReturnProfile } from "@/lib/types";
import * as api from "@/lib/api";
import type { ProfilePayload } from "@/lib/api";
import { AppShell } from "@/components/layout/AppShell";
import { ProfileForm } from "@/components/profiles/ProfileForm";
import { ProfileCard } from "@/components/profiles/ProfileCard";
import { MappingsPanel } from "@/components/profiles/MappingsPanel";
import { DeleteDialog } from "@/components/accounts/DeleteDialog";

export default function ProfilesPage() {
  const [profiles, setProfiles] = useState<ReturnProfile[]>([]);
  const [mappings, setMappings] = useState<AssetMapping[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [loading, setLoading] = useState(true);

  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<ReturnProfile | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ReturnProfile | null>(null);

  const load = useCallback(async () => {
    try {
      const [p, m, a] = await Promise.all([
        api.getProfiles(),
        api.getMappings(),
        api.getAccounts(),
      ]);
      setProfiles(p);
      setMappings(m);
      setAccounts(a);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  async function handleCreate(data: ProfilePayload) {
    await api.createProfile(data);
    setShowForm(false);
    await load();
  }

  async function handleEdit(data: ProfilePayload) {
    if (!editing) return;
    await api.updateProfile(editing.id, data);
    setEditing(null);
    await load();
  }

  async function handleDelete() {
    if (!deleteTarget) return;
    await api.deleteProfile(deleteTarget.id);
    setDeleteTarget(null);
    await load();
  }

  if (loading) {
    return (
      <AppShell>
        <div className="text-gray-500">Loading profiles...</div>
      </AppShell>
    );
  }

  return (
    <AppShell>
      <div>
        {deleteTarget && (
          <DeleteDialog
            name={deleteTarget.name}
            onConfirm={handleDelete}
            onCancel={() => setDeleteTarget(null)}
          />
        )}

        <div className="flex items-center justify-between mb-6">
          <div>
            <h1 className="text-2xl font-bold text-gray-900">Return Profiles</h1>
            <p className="text-gray-500 mt-1">
              Define how assets grow over time and map them to your holdings.
            </p>
          </div>
          {!showForm && !editing && (
            <button
              onClick={() => setShowForm(true)}
              className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 transition-colors"
            >
              Add Profile
            </button>
          )}
        </div>

        {showForm && (
          <div className="mb-6 p-6 bg-white border border-gray-200 rounded-lg">
            <ProfileForm
              onSubmit={handleCreate}
              onCancel={() => setShowForm(false)}
            />
          </div>
        )}

        {editing && (
          <div className="mb-6 p-6 bg-white border border-gray-200 rounded-lg">
            <ProfileForm
              initial={editing}
              onSubmit={handleEdit}
              onCancel={() => setEditing(null)}
            />
          </div>
        )}

        <section className="mb-10">
          {profiles.length === 0 ? (
            <div className="text-center py-12 text-gray-500">
              <p className="text-lg">No profiles yet</p>
              <p className="mt-1 text-sm">
                Create your first return profile to model asset growth.
              </p>
            </div>
          ) : (
            <div className="space-y-2">
              {profiles.map((p) => (
                <ProfileCard
                  key={p.id}
                  profile={p}
                  onEdit={() => {
                    setShowForm(false);
                    setEditing(p);
                  }}
                  onDelete={() => setDeleteTarget(p)}
                />
              ))}
            </div>
          )}
        </section>

        <section>
          <h2 className="text-xl font-semibold text-gray-900 mb-4">Mappings</h2>
          <MappingsPanel
            accounts={accounts}
            profiles={profiles}
            mappings={mappings}
            onChanged={load}
          />
        </section>
      </div>
    </AppShell>
  );
}
