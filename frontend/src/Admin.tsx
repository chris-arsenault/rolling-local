import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import type { Auth0Client } from "@auth0/auth0-spa-js";
import { ApiError, errorMessage, request, statusLabel } from "./operatorApi";
import type { Listing, Registration } from "./operatorApi";

type Truck = Registration & {
  version: string;
  operatorSubject: string | null;
  review: { by: string; at: string } | null;
};
type Change = { listing: Listing } | { decision: "accept" | "reject" }
  | { operatorSubject: string; confirmOwnership: true };

function truckListing(form: FormData): Listing {
  return { name: String(form.get("name")).trim(), cuisine: String(form.get("cuisine")).trim() };
}

function ListingFields({ listing }: { listing?: Listing }) {
  return <>
    <label>Truck name<input name="name" defaultValue={listing?.name} required maxLength={100} /></label>
    <label>Cuisine<input name="cuisine" defaultValue={listing?.cuisine} required maxLength={100} /></label>
  </>;
}

function TruckEditor({ truck, busy, update }: { truck: Truck; busy: boolean; update: (change: Change) => void }) {
  const [decision, setDecision] = useState<"accept" | "reject" | null>(null);
  function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    update({ listing: truckListing(new FormData(event.currentTarget)) });
  }
  function link(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    update({ operatorSubject: String(form.get("subject")).trim(), confirmOwnership: true });
  }
  return (
    <section className="operator-card truck-editor">
      <span className="status" data-status={truck.status}>{statusLabel[truck.status]}</span>
      <h2>{truck.name}</h2>
      <p className="muted">{truck.operatorSubject ? `Operator: ${truck.operatorSubject}` : "Unassigned"}</p>
      {truck.review && <p className="muted">Reviewed {new Date(truck.review.at).toLocaleString()} by {truck.review.by}</p>}
      {truck.status === "pending" && (decision ? <div>
        <p>{decision === "accept" ? "Accept this truck and publish its listing?" : "Reject this truck and keep its listing private?"}</p>
        <div className="form-actions">
          <button className={decision === "accept" ? "primary-button" : "reject-button"} disabled={busy}
            onClick={() => { update({ decision }); setDecision(null); }}>
            {decision === "accept" ? "Confirm acceptance" : "Confirm rejection"}
          </button>
          <button disabled={busy} onClick={() => setDecision(null)}>Cancel</button>
        </div>
      </div> : <div className="form-actions">
        <button className="primary-button" disabled={busy} onClick={() => setDecision("accept")}>Accept</button>
        <button className="reject-button" disabled={busy} onClick={() => setDecision("reject")}>Reject</button>
      </div>)}
      <form className="operator-form" onSubmit={save}>
        <ListingFields listing={truck} />
        <button type="submit" disabled={busy}>Save listing</button>
      </form>
      {!truck.operatorSubject && truck.status !== "rejected" && <details>
        <summary>Assign an operator</summary>
        <form className="operator-form" onSubmit={link}>
          <p>Ask the operator to sign in from their registration link and share the account ID shown there.</p>
          <label>Auth0 user ID<input name="subject" required maxLength={255} /></label>
          <label className="checkbox"><input type="checkbox" required />I confirmed this account belongs to the operator.</label>
          <button type="submit" disabled={busy}>Assign operator</button>
        </form>
      </details>}
    </section>
  );
}

export default function Admin({ client }: { client: Auth0Client }) {
  const [trucks, setTrucks] = useState<Truck[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [newId, setNewId] = useState(() => crypto.randomUUID());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  useEffect(() => {
    let active = true;
    request<{ trucks: Truck[] }>(client, "/admin/trucks").then(
      (result) => { if (active) setTrucks(result.trucks); },
      (failure) => { if (active) setError(errorMessage(failure)); },
    );
    return () => { active = false; };
  }, [client]);

  async function save(path: string, method: "POST" | "PUT", data: object) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const result = await request<Truck>(client, path, method, data);
      setTrucks((current) => [...(current ?? []).filter((truck) => truck.id !== result.id), result]);
      setSelected(result.id);
      setCreating(false);
      setNewId(crypto.randomUUID());
      setNotice(method === "POST" ? "Truck added." : "Truck updated.");
    } catch (failure) {
      setError(failure instanceof ApiError && failure.status === 409
        ? "This listing or account changed. Reload the page to review the current records."
        : errorMessage(failure));
    } finally {
      setBusy(false);
    }
  }

  function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    void save("/admin/trucks", "POST", { ...truckListing(form), publish: form.has("publish"), id: newId });
  }

  const current = trucks?.find((truck) => truck.id === selected);
  const ordered = [...(trucks ?? [])].sort((left, right) => (
    Number(right.status === "pending") - Number(left.status === "pending") || left.name.localeCompare(right.name)
  ));

  return <>
    {error && <p role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {!trucks ? (!error && <p role="status">Loading truck listings…</p>) : (
      <div className="admin-workspace">
        <section className="operator-card" aria-labelledby="admin-list-title">
          <h2 id="admin-list-title">Trucks</h2>
          <button className="primary-button" disabled={busy} onClick={() => setCreating(true)}>Add a truck</button>
          <p className="muted">{trucks.filter((truck) => truck.status === "pending").length} pending review</p>
          {ordered.length > 0 ? <ul className="admin-list">
            {ordered.map((truck) => <li key={truck.id}>
              <button type="button" disabled={busy} aria-pressed={!creating && selected === truck.id}
                onClick={() => { setCreating(false); setSelected(truck.id); }}>
                <strong>{truck.name}</strong>
                <span className="status" data-status={truck.status}>{statusLabel[truck.status]}</span>
              </button>
            </li>)}
          </ul> : <p>No trucks yet.</p>}
        </section>
        {creating ? <section className="operator-card">
          <h2>Add a truck</h2>
          <form className="operator-form" onSubmit={create}>
            <ListingFields />
            <label className="checkbox"><input type="checkbox" name="publish" />Publish this listing now</label>
            <p>The listing will stay private until accepted if you leave this unchecked.</p>
            <div className="form-actions">
              <button type="submit" className="primary-button" disabled={busy}>Add truck</button>
              <button type="button" disabled={busy} onClick={() => setCreating(false)}>Cancel</button>
            </div>
          </form>
        </section> : current ? <TruckEditor key={current.version} truck={current} busy={busy}
          update={(change) => void save(`/admin/trucks/${current.id}`, "PUT", { version: current.version, ...change })} />
          : <p className="muted">Select a truck to review or edit its listing.</p>}
      </div>
    )}
  </>;
}
