import { Auth0Client } from "@auth0/auth0-spa-js";
import { apiUrl } from "./directory";

export type OperatorRoute = "/register" | "/admin";
export type TruckStatus = "pending" | "approved" | "rejected";
export type Listing = { name: string; cuisine: string };
export type Registration = Listing & { id: string; status: TruckStatus };
export type Account = { accountId: string; registration: Registration | null };

type LoginConfig = { issuer: string; audience: string; connection: string; webClientId: string };

export class ApiError extends Error {
  constructor(public readonly status: number, message: string) {
    super(message);
  }
}

export async function operatorClient(route: OperatorRoute) {
  const response = await fetch(apiUrl("/public/config"), { credentials: "omit", cache: "no-store" });
  if (!response.ok) throw new Error("Sign-in is unavailable. Try again shortly.");
  const config: LoginConfig = await response.json();
  if (!config.issuer || !config.audience || !config.connection || !config.webClientId) {
    throw new Error("Sign-in is unavailable. Try again shortly.");
  }
  const client = new Auth0Client({
    domain: new URL(config.issuer).host,
    clientId: config.webClientId,
    authorizationParams: {
      audience: config.audience,
      connection: config.connection,
      redirect_uri: new URL(route, window.location.origin).href,
    },
  });
  let callbackError: string | undefined;
  const query = new URLSearchParams(window.location.search);
  if (query.has("state") && (query.has("code") || query.has("error"))) {
    try {
      await client.handleRedirectCallback();
    } catch {
      callbackError = "Sign-in did not finish. Sign in again to continue.";
    } finally {
      window.history.replaceState(null, "", route);
    }
  } else {
    await client.checkSession();
  }
  return { client, signedIn: await client.isAuthenticated(), callbackError };
}

export async function request<T>(client: Auth0Client, path: string, method = "GET", body?: object): Promise<T> {
  let token: string | undefined;
  try {
    token = await client.getTokenSilently();
  } catch {
    throw new ApiError(401, "Your session ended. Sign in again to continue.");
  }
  if (!token) throw new ApiError(401, "Your session ended. Sign in again to continue.");
  const headers = new Headers({ Authorization: `Bearer ${token}` });
  if (body) headers.set("Content-Type", "application/json");
  const response = await fetch(apiUrl(path), {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
    credentials: "omit",
    cache: "no-store",
  });
  if (!response.ok) {
    const result: { error?: string } = await response.json().catch(() => ({}));
    throw new ApiError(response.status, result.error ?? "The request could not be completed.");
  }
  return response.json();
}

export const statusLabel: Record<TruckStatus, string> = {
  pending: "Pending review",
  approved: "Approved",
  rejected: "Rejected",
};

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "The request could not be completed.";
}
