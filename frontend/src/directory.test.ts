import { describe, expect, it } from "vitest";
import { eligible, filterDirectory, offline, type Truck } from "./directory";

const truck: Truck = {
  id: "truck-a",
  name: "Cañon Kitchen",
  cuisine: "Street tacos",
  active: true,
  location: {
    position: { latitude: 34.54, longitude: -112.47 },
    recordedAt: "2026-11-17T18:30:00Z",
    expiresAt: "2026-11-17T18:50:00Z",
  },
  menu: { id: "menu-a", url: "https://rolling-local.test/menu.jpg", capturedAt: "2026-11-17T18:00:00Z", publishedAt: "2026-11-17T18:01:00Z" },
};

describe("public discovery", () => {
  it("filters name and cuisine while keeping offline trucks searchable", () => {
    expect(filterDirectory([offline(truck)], "canon").map((entry) => entry.id)).toEqual(["truck-a"]);
    expect(filterDirectory([truck], "canon tacos")).toEqual([]);
    expect(filterDirectory([truck], "ramen")).toEqual([]);
  });

  it("finds one current item by phrase, exact tag or alias and retains its price", () => {
    const items = [
      { id: "a", name: "Sonoran hot dog", searchName: "sonoran hot dog", description: null, searchDescription: null, priceText: "$7", tagIds: [] },
      { id: "b", name: "Spicy tuna roll", searchName: "spicy tuna roll", description: null, searchDescription: null, priceText: null, tagIds: ["sushi", "hot"] },
    ];
    const catalog = { menus: new Map([[truck.id, {truckId: truck.id, menuId: "menu-a", items}]]),
      tags: [{ id: "sushi", label: "Sushi", aliases: [] }, {id: "hot", label: "Hot / spicy", aliases: ["spicy"]}] };
    for (const [query, index] of [["HOT  DOG", 0], ["sushi", 1], ["spicy", 1]] as const)
      expect(filterDirectory([offline(truck)], query, catalog)[0].matchedItems).toEqual([items[index]]);
    expect(filterDirectory([truck], "dog spicy", catalog)).toEqual([]);
    expect(filterDirectory([truck], "sush", catalog)).toEqual([]);
    expect(filterDirectory([{...truck, menu: null}], "sushi", catalog)).toEqual([]);
  });

  it("drops a position at its deadline", () => {
    const expires = Date.parse("2026-11-17T18:50:00Z");
    expect(eligible(truck, expires - 1)).toEqual(truck.location);
    expect(eligible(truck, expires)).toBeUndefined();
  });

  it("retains the menu and drops coordinates when updates fail", () => {
    expect(offline(truck)).not.toHaveProperty("location");
    expect(offline(truck).menu).toEqual(truck.menu);
  });
});
