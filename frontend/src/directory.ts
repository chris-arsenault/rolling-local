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
  menu: { id: string; url: string; capturedAt: string; publishedAt: string } | null;
};

export type MenuItem = {
  id: string; name: string; description: string | null; priceText: string | null; tagIds: string[];
  searchName: string; searchDescription: string | null;
};
type CatalogMenu = { truckId: string; menuId: string; items: MenuItem[] };
type Tag = { id: string; label: string; aliases: string[] };
export type Catalog = { menus: Map<string, CatalogMenu>; tags: Tag[] };

declare global {
  interface Window {
    __APP_CONFIG__?: { apiBaseUrl?: string };
  }
}

const defaultApi = "https://api.rolling-local.ahara.io";
const searchKey = (text: string) =>
  text.normalize("NFD").replaceAll(/\p{M}/gu, "").toLowerCase().trim().replaceAll(/\s+/gu, " ");

export function apiUrl(path: string): URL {
  return new URL(path, window.__APP_CONFIG__?.apiBaseUrl ?? defaultApi);
}

export function eligible(truck: Truck, now: number): Location | undefined {
  return truck.active && truck.location && Date.parse(truck.location.expiresAt) > now
    ? truck.location
    : undefined;
}

export function filterDirectory(trucks: Truck[], query: string, catalog?: Catalog) {
  const phrase = searchKey(query);
  const tagIds = new Set(catalog?.tags.filter(tag =>
    [tag.id, tag.label, ...tag.aliases].some(label => searchKey(label) === phrase)).map(tag => tag.id));
  return trucks.flatMap(truck => {
    const menu = catalog?.menus.get(truck.id);
    const matchedItems = phrase && menu && truck.menu?.id === menu.menuId
      ? menu.items.filter(item => item.searchName.includes(phrase)
          || item.searchDescription?.includes(phrase) || item.tagIds.some(tag => tagIds.has(tag)))
      : [];
    return matchedItems.length || [truck.name, truck.cuisine].some(field => searchKey(field).includes(phrase))
      ? [{ ...truck, matchedItems }] : [];
  })
    .sort((left, right) => left.name.localeCompare(right.name));
}

export async function loadCatalog(signal: AbortSignal): Promise<Catalog> {
  const menus = new Map<string, CatalogMenu>();
  let tags: Tag[] = [];
  let after: string | null = null;
  do {
    const url = apiUrl("/public/menu-catalog");
    if (after) url.searchParams.set("after", after);
    const response = await fetch(url, { signal, cache: "no-store", credentials: "omit" });
    if (!response.ok) throw new Error("Menu catalog unavailable");
    const page: { menus: CatalogMenu[]; tags: Tag[]; next: string | null } = await response.json();
    for (const menu of page.menus) menus.set(menu.truckId, menu);
    tags = page.tags;
    after = page.next;
  } while (after);
  return { menus, tags };
}

export function offline(truck: Truck): Truck {
  return { id: truck.id, name: truck.name, cuisine: truck.cuisine, menu: truck.menu, active: false };
}

export async function loadDirectory(signal: AbortSignal): Promise<Truck[]> {
  const response = await fetch(apiUrl("/public/trucks"), {
    signal,
    cache: "no-store",
    credentials: "omit",
  });
  if (!response.ok) throw new Error("Directory request failed");
  const result: { trucks?: Truck[] } = await response.json();
  if (!Array.isArray(result.trucks)) throw new Error("Invalid directory response");
  return result.trucks;
}
