"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useAuth } from "@/lib/auth";

const NAV_ITEMS = [
  { href: "/accounts", label: "Accounts" },
];

export function Sidebar() {
  const pathname = usePathname();
  const router = useRouter();
  const { user, logout } = useAuth();

  async function handleLogout() {
    await logout();
    router.push("/login");
  }

  return (
    <aside className="fixed left-0 top-0 h-full w-56 bg-gray-900 text-white flex flex-col">
      <div className="p-5 border-b border-gray-700">
        <Link href="/accounts" className="text-xl font-bold tracking-tight">
          FinPlan
        </Link>
      </div>
      <nav className="flex-1 p-3 space-y-1">
        {NAV_ITEMS.map(({ href, label }) => {
          const active = pathname.startsWith(href);
          return (
            <Link
              key={href}
              href={href}
              className={`block px-3 py-2 rounded-md text-sm font-medium transition-colors ${
                active
                  ? "bg-gray-700 text-white"
                  : "text-gray-300 hover:bg-gray-800 hover:text-white"
              }`}
            >
              {label}
            </Link>
          );
        })}
      </nav>
      {user && (
        <div className="p-4 border-t border-gray-700">
          <p className="text-sm text-gray-400 truncate">{user.email}</p>
          <button
            onClick={handleLogout}
            className="mt-2 text-sm text-gray-400 hover:text-white transition-colors"
          >
            Log out
          </button>
        </div>
      )}
    </aside>
  );
}
