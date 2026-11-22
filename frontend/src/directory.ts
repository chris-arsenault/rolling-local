export type Location = {
  position: { latitude: number; longitude: number };
  recordedAt: string;
  expiresAt: string;
};

export type Truck = {
  id: string;
  name: string;
  cuisine: string;
  active: boolean;
  location?: Location;
  menu: { url: string; capturedAt: string; publishedAt: string } | null;
};

declare global {
  interface Window {
    __APP_CONFIG__?: { apiBaseUrl?: string };
  }
}

const defaultApi = "https://api.rolling-local.ahara.io";
const searchKey = (text: string) => text.normalize("NFD").replaceAll(/\p{M}/gu, "").toLowerCase();

export function eligible(truck: Truck, now: number): Location | undefined {
  return truck.active && truck.location && Date.parse(truck.location.expiresAt) > now
    ? truck.location
    : undefined;
}

export function filterDirectory(trucks: Truck[], query: string): Truck[] {
  const words = searchKey(query).trim().split(/\s+/);
  return trucks
    .filter((truck) => {
      const text = searchKey(`${truck.name} ${truck.cuisine}`);
      return words.every((word) => text.includes(word));
    })
    .sort((left, right) => left.name.localeCompare(right.name));
}

export function offline(truck: Truck): Truck {
  return { id: truck.id, name: truck.name, cuisine: truck.cuisine, menu: truck.menu, active: false };
}

export async function loadDirectory(signal: AbortSignal): Promise<Truck[]> {
  const endpoint = window.__APP_CONFIG__?.apiBaseUrl ?? import.meta.env.VITE_API_ORIGIN ?? defaultApi;
  const response = await fetch(new URL("/public/trucks", endpoint), {
    signal,
    cache: "no-store",
    credentials: "omit",
  });
  if (!response.ok) throw new Error("Directory request failed");
  const result: { trucks?: Truck[] } = await response.json();
  if (!Array.isArray(result.trucks)) throw new Error("Invalid directory response");
  return result.trucks;
}
