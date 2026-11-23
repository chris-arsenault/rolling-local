import { lazy, Suspense, useEffect, useState } from "react";
import type { FormEvent } from "react";
import type { Auth0Client } from "@auth0/auth0-spa-js";
import { errorMessage, operatorClient, request, statusLabel } from "./operatorApi";
import type { Account, Listing, OperatorRoute } from "./operatorApi";
import "./operator.css";

const Admin = lazy(() => import("./Admin"));
type Session = Awaited<ReturnType<typeof operatorClient>>;
type LoginState = { session: Session | null; error: string };

const registrationMessage = {
  pending: "We’ll review your application before your truck appears in the directory. Check this page for the decision.",
  approved: "Your truck is listed in the directory. Open the operator app to set your operating hours and share your location or menu.",
  rejected: "Your registration was declined. Your truck is private.",
};

function RegistrationForm({ client }: { client: Auth0Client }) {
  const [account, setAccount] = useState<Account | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    request<Account>(client, "/operator").then(
      (result) => { if (active) { setAccount(result); setBusy(false); } },
      (failure) => { if (active) { setError(errorMessage(failure)); setBusy(false); } },
    );
    return () => { active = false; };
  }, [client]);

  async function load(listing?: Listing) {
    setBusy(true);
    setError("");
    try {
      const result = await (listing
        ? request<Account>(client, "/operator/registration", "POST", listing)
        : request<Account>(client, "/operator"));
      setAccount(result);
    } catch (failure) {
      setError(errorMessage(failure));
    } finally {
      setBusy(false);
    }
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    void load({ name: String(form.get("name")).trim(), cuisine: String(form.get("cuisine")).trim() });
  }

  const registration = account?.registration;
  return (
    <section className="operator-card">
      {error && <p role="alert">{error}</p>}
      {registration ? <>
        <span className="status" data-status={registration.status}>{statusLabel[registration.status]}</span>
        <h2>{registration.name}</h2>
        <p>{registrationMessage[registration.status]}</p>
        {registration.status === "pending" && <button type="button" disabled={busy} onClick={() => void load()}>Check status</button>}
      </> : account ? (
        <form onSubmit={submit} className="operator-form">
          <label>Truck name<input name="name" autoComplete="organization" maxLength={100} required /></label>
          <label>Cuisine<input name="cuisine" maxLength={100} required /></label>
          <p>Your truck stays private while we review your registration.</p>
          <button className="primary-button" disabled={busy}>Submit registration</button>
        </form>
      ) : busy ? <p role="status">Loading your registration…</p>
        : <button type="button" onClick={() => void load()}>Try again</button>}
      {account && <details className="account-details">
        <summary>Account ID</summary>
        <p>Share this ID with Rolling Local if the owner is linking an existing truck to your account.</p>
        <code>{account.accountId}</code>
      </details>}
    </section>
  );
}

export default function OperatorAccess({ route }: { route: OperatorRoute }) {
  const [login, setLogin] = useState<LoginState | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const admin = route === "/admin";
  const title = admin ? "Truck administration" : "Register your truck";

  useEffect(() => {
    document.title = `${title} · Rolling Local`;
    let active = true;
    operatorClient(route).then(
      (session) => { if (active) setLogin({ session, error: session.callbackError ?? "" }); },
      (failure) => { if (active) setLogin({ session: null, error: errorMessage(failure) }); },
    );
    return () => { active = false; };
  }, [route, title]);

  const client = login?.session?.client;
  const signedIn = login?.session?.signedIn;
  async function authenticate(action: "signin" | "signup" | "signout") {
    if (!client) return;
    setError("");
    setWorking(true);
    try {
      if (action === "signout") {
        await client.logout({ logoutParams: { returnTo: new URL(route, window.location.origin).href } });
      } else {
        await client.loginWithRedirect({ authorizationParams: { screen_hint: action === "signup" ? "signup" : "login" } });
      }
    } catch (failure) {
      setError(errorMessage(failure));
      setWorking(false);
    }
  }

  return (
    <main className={admin ? "operator-page admin-page" : "operator-page"}>
      <header className="operator-header">
        <a className="brand" href="/">Rolling<span>Local</span></a>
        {client && signedIn && <button disabled={working} onClick={() => void authenticate("signout")}>Sign out</button>}
      </header>
      <h1>{title}</h1>
      {(error || login?.error) && <p role="alert">{error || login?.error}</p>}
      {!login ? <p role="status">Loading sign-in…</p>
        : !client ? <button onClick={() => window.location.reload()}>Try again</button>
        : !signedIn ? <section className="operator-card">
          <p>{admin ? "Sign in with the owner account to review registrations and manage truck listings." : "Sign in with your Ahara account, or create one, to put your truck in front of hungry customers. We review each registration before it goes public."}</p>
          <div className="form-actions">
            <button className="primary-button" disabled={working} onClick={() => void authenticate("signin")}>Sign in with Ahara</button>
            {!admin && <button disabled={working} onClick={() => void authenticate("signup")}>Create an account</button>}
          </div>
        </section>
        : admin ? <Suspense fallback={<p role="status">Loading administration…</p>}><Admin client={client} /></Suspense>
        : <RegistrationForm client={client} />}
    </main>
  );
}
