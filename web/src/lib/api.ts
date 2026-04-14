import type {
  Account,
  AllocationData,
  AssetMapping,
  Holding,
  ReturnProfile,
  ReturnProfileType,
  User,
} from "./types";

const BASE = "/api";

async function request<T>(
  path: string,
  options?: RequestInit
): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    ...options,
  });
  if (!res.ok) {
    if (res.status === 401 && typeof window !== "undefined" && !path.startsWith("/auth/")) {
      window.location.href = "/login";
      throw new Error("Unauthorized");
    }
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || `Request failed: ${res.status}`);
  }
  if (res.status === 204) return undefined as T;
  return res.json();
}

// Auth
export const register = (email: string, password: string) =>
  request<User>("/auth/register", {
    method: "POST",
    body: JSON.stringify({ email, password }),
  });

export const login = (email: string, password: string) =>
  request<User>("/auth/login", {
    method: "POST",
    body: JSON.stringify({ email, password }),
  });

export const logout = () =>
  request<void>("/auth/logout", { method: "POST" });

export const getMe = () => request<User>("/auth/me");

// Accounts
export const getAccounts = () => request<Account[]>("/accounts");

export const getAccount = (id: number) => request<Account>(`/accounts/${id}`);

export const createAccount = (data: {
  name: string;
  description?: string;
  account_type: string;
  value?: number;
  return_profile?: string;
  balance?: number;
  interest_rate?: number;
}) => request<Account>("/accounts", { method: "POST", body: JSON.stringify(data) });

export const updateAccount = (
  id: number,
  data: {
    name?: string;
    description?: string;
    value?: number;
    return_profile?: string;
    balance?: number;
    interest_rate?: number;
  }
) => request<Account>(`/accounts/${id}`, { method: "PUT", body: JSON.stringify(data) });

export const deleteAccount = (id: number) =>
  request<void>(`/accounts/${id}`, { method: "DELETE" });

// Holdings
export const getHoldings = (accountId: number) =>
  request<Holding[]>(`/accounts/${accountId}/holdings`);

export const createHolding = (
  accountId: number,
  data: { asset_name: string; value: number }
) =>
  request<Holding>(`/accounts/${accountId}/holdings`, {
    method: "POST",
    body: JSON.stringify(data),
  });

export const updateHolding = (
  holdingId: number,
  data: { asset_name?: string; value?: number }
) =>
  request<Holding>(`/holdings/${holdingId}`, {
    method: "PUT",
    body: JSON.stringify(data),
  });

export const deleteHolding = (holdingId: number) =>
  request<void>(`/holdings/${holdingId}`, { method: "DELETE" });

// Return profiles
export interface ProfilePayload {
  name: string;
  description?: string;
  profile_type: ReturnProfileType;
  rate?: number;
  mean?: number;
  std_dev?: number;
  scale?: number;
  df?: number;
  preset?: string;
  block_size?: number;
}

export const getProfiles = () => request<ReturnProfile[]>("/profiles");

export const createProfile = (data: ProfilePayload) =>
  request<ReturnProfile>("/profiles", {
    method: "POST",
    body: JSON.stringify(data),
  });

export const updateProfile = (id: number, data: ProfilePayload) =>
  request<ReturnProfile>(`/profiles/${id}`, {
    method: "PUT",
    body: JSON.stringify(data),
  });

export const deleteProfile = (id: number) =>
  request<void>(`/profiles/${id}`, { method: "DELETE" });

// Asset mappings
export const getMappings = () => request<AssetMapping[]>("/mappings");

export const upsertMapping = (assetName: string, profileId: number) =>
  request<AssetMapping>(`/mappings/${encodeURIComponent(assetName)}`, {
    method: "PUT",
    body: JSON.stringify({ profile_id: profileId }),
  });

export const deleteMapping = (assetName: string) =>
  request<void>(`/mappings/${encodeURIComponent(assetName)}`, {
    method: "DELETE",
  });

// Allocation
export const getAllocation = () => request<AllocationData>("/allocation");
