"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { useAuth } from "@/lib/auth";
import { AuthForm } from "@/components/auth/AuthForm";

export default function LoginPage() {
  const [mode, setMode] = useState<"login" | "register">("login");
  const { login, register } = useAuth();
  const router = useRouter();

  async function handleSubmit(email: string, password: string) {
    if (mode === "login") {
      await login(email, password);
    } else {
      await register(email, password);
    }
    router.push("/accounts");
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-50">
      <div className="w-full max-w-sm">
        <h1 className="text-2xl font-bold text-center text-gray-900 mb-8">
          FinPlan
        </h1>
        <div className="bg-white p-6 rounded-lg border border-gray-200 shadow-sm">
          <h2 className="text-lg font-semibold text-gray-900 mb-4">
            {mode === "login" ? "Welcome back" : "Create your account"}
          </h2>
          <AuthForm
            mode={mode}
            onSubmit={handleSubmit}
            onToggleMode={() =>
              setMode(mode === "login" ? "register" : "login")
            }
          />
        </div>
      </div>
    </div>
  );
}
