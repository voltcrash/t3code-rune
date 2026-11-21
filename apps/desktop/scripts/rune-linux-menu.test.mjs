import * as NodeEvents from "node:events";
import dbus from "dbus-next";
import { expect, it, vi } from "vitest";
import { registerWindow } from "./rune-linux-menu.mjs";

it("registers with the current panel across a late owner lookup and releases a disconnected window", async () => {
  const lookup = Promise.withResolvers();
  const pending = Promise.withResolvers();
  const published = Promise.withResolvers();
  const bus = Object.assign(new NodeEvents.EventEmitter(), {
    call: async (message) => {
      if (message.member === "GetNameOwner") {
        pending.resolve();
        return lookup.promise;
      }
    },
    send: vi.fn((message) => {
      if (message.member === "UnregisterWindow") throw new Error("closed bus");
      published.resolve(message);
    }),
    export: vi.fn(),
    disconnect: vi.fn(),
  });
  using _bus = vi.spyOn(dbus, "sessionBus").mockReturnValue(bus);
  const controller = new AbortController();
  const registration = registerWindow(15, "en", controller.signal, vi.fn());
  await pending.promise;
  bus.emit("message", {
    type: dbus.MessageType.SIGNAL,
    sender: "org.freedesktop.DBus",
    interface: "org.freedesktop.DBus",
    member: "NameOwnerChanged",
    body: ["com.canonical.AppMenu.Registrar", ":1.3", ":1.5"],
  });
  lookup.resolve({ body: [":1.3"] });
  await registration;
  expect(await published.promise).toMatchObject({
    destination: ":1.5",
    body: [15, "/com/canonical/menu/15"],
  });
  expect(bus.send).toHaveBeenCalledTimes(1);
  const menu = bus.export.mock.calls[0][1];
  expect(menu.AboutToShowGroup([0, -1])).toEqual([[], [-1]]);
  expect(() => menu.EventGroup([[-1, "clicked"]])).toThrow(dbus.DBusError);

  controller.abort();
  expect(bus.send.mock.lastCall[0]).toMatchObject({
    member: "UnregisterWindow",
    destination: ":1.5",
    body: [15],
  });
  expect(bus.disconnect).toHaveBeenCalledOnce();
});
