"use client";

import type { ReactNode } from "react";
import { Sidebar } from "./Sidebar";
import { useAuth } from "@/lib/auth";

export function AppShell({ children }: { children: ReactNode }) {
  const { user, loading } = useAuth();

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-screen text-gray-500">
        Loading...
      </div>
    );
  }

  if (!user) {
    return null; // AuthProvider will redirect to /login
  }

  return (
    <div className="flex min-h-full">
      <Sidebar />
      <main className="flex-1 ml-56 p-8">{children}</main>
    </div>
  );
}
