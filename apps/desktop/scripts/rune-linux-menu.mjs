import dbus from "dbus-next";
import { msg } from "gt-i18n";
import { m, selectLanguage } from "../src/i18n/catalog.ts";

const REGISTRAR = "com.canonical.AppMenu.Registrar";
const REGISTRAR_PATH = "/com/canonical/AppMenu/Registrar";
const DBUS = "org.freedesktop.DBus";
const MENU = "com.canonical.dbusmenu";
const { DBusError, Message, Variant } = dbus;

function menuItems() {
  const items = [];
  const add = (properties, children = [], action) => {
    const id = items.length;
    items.push({ id, properties, children, action });
    return id;
  };
  const submenu = (label, children) =>
    add(
      { label: new Variant("s", m(label)), "children-display": new Variant("s", "submenu") },
      children,
    );
  const item = (action, label, ...shortcut) =>
    add(
      {
        label: new Variant("s", m(label)),
        ...(shortcut.length > 0 && { shortcut: new Variant("aas", [shortcut]) }),
      },
      [],
      action,
    );
  const separator = () => add({ type: new Variant("s", "separator") });
  const children = [];
  add({ "children-display": new Variant("s", "submenu") }, children);
  children.push(
    submenu(msg("File", { $context: "menu bar title" }), [
      item("settings", msg("Settings..."), "Control", "comma"),
      separator(),
      item("quit", msg("Quit"), "Control", "q"),
    ]),
    submenu(msg("Edit", { $context: "menu bar title" }), [
      item("undo", msg("Undo", { $context: "Edit menu item" }), "Control", "z"),
      item("redo", msg("Redo", { $context: "Edit menu item" }), "Control", "Shift", "z"),
      separator(),
      item("cut", msg("Cut", { $context: "Edit menu item" }), "Control", "x"),
      item("copy", msg("Copy", { $context: "Edit menu item" }), "Control", "c"),
      item("paste", msg("Paste", { $context: "Edit menu item" }), "Control", "v"),
      item("paste", msg("Paste as Text"), "Control", "Shift", "v"),
      item("delete", msg("Delete", { $context: "Edit menu item" })),
      separator(),
      item("select-all", msg("Select All", { $context: "Edit menu item" }), "Control", "a"),
    ]),
    submenu(msg("View", { $context: "menu bar title" }), [
      item("actual-size", msg("Actual Size"), "Control", "0"),
      item("zoom-in", msg("Zoom In"), "Control", "equal"),
      item("zoom-out", msg("Zoom Out"), "Control", "minus"),
      separator(),
      item("fullscreen", msg("Toggle Full Screen"), "F11"),
    ]),
    submenu(msg("Window", { $context: "menu bar title" }), [
      item("minimize", msg("Minimize", { $context: "Window menu item" }), "Control", "m"),
      item("maximize", msg("Zoom", { $context: "Window menu item, resizes the window" })),
    ]),
  );
  return items;
}

class ApplicationMenu extends dbus.interface.Interface {
  Version = 4;
  TextDirection = "ltr";
  Status = "normal";
  IconThemePath = [];

  constructor(activate) {
    super(MENU);
    this.activate = activate;
    this.items = menuItems();
  }

  item(id) {
    const item = this.items[id];
    if (!item) throw new DBusError(MENU + ".Error.InvalidMenuitem", "Unknown menu item");
    return item;
  }

  properties(item, names) {
    if (!names.length) return item.properties;
    return Object.fromEntries(
      Object.entries(item.properties).filter(([name]) => names.includes(name)),
    );
  }

  GetLayout(parent, depth, names) {
    const layout = (id, remaining) => {
      const item = this.item(id);
      return [
        id,
        this.properties(item, names),
        remaining === 0
          ? []
          : item.children.map((child) => new Variant("(ia{sv}av)", layout(child, remaining - 1))),
      ];
    };
    return [1, layout(parent, depth)];
  }

  GetGroupProperties(ids, names) {
    const items = ids.length ? ids.map((id) => this.items[id]).filter(Boolean) : this.items;
    return items.map((item) => [item.id, this.properties(item, names)]);
  }

  GetProperty(id, name) {
    const props = this.item(id).properties;
    if (Object.hasOwn(props, name)) return props[name];
    if (name === "enabled" || name === "visible") return new Variant("b", true);
    throw new DBusError(MENU + ".Error.UnknownProperty", "Unknown menu item property");
  }

  Event(id, event) {
    const item = this.item(id);
    if (event === "clicked" && item.action) this.activate(item.action);
  }

  EventGroup(events) {
    const errors = [];
    for (const [id, event] of events) {
      if (this.items[id]) this.Event(id, event);
      else errors.push(id);
    }
    return this.groupErrors(events.length, errors);
  }

  groupErrors(count, errors) {
    if (errors.length && errors.length === count) this.item(errors[0]);
    return errors;
  }

  AboutToShow(id) {
    this.item(id);
    return false;
  }

  AboutToShowGroup(ids) {
    return [
      [],
      this.groupErrors(
        ids.length,
        ids.filter((id) => !this.items[id]),
      ),
    ];
  }
}

ApplicationMenu.configureMembers({
  properties: {
    Version: { signature: "u", access: "read" },
    TextDirection: { signature: "s", access: "read" },
    Status: { signature: "s", access: "read" },
    IconThemePath: { signature: "as", access: "read" },
  },
  methods: {
    GetLayout: { inSignature: "iias", outSignature: "u(ia{sv}av)" },
    GetGroupProperties: { inSignature: "aias", outSignature: "a(ia{sv})" },
    GetProperty: { inSignature: "is", outSignature: "v" },
    Event: { inSignature: "isvu" },
    EventGroup: { inSignature: "a(isvu)", outSignature: "ai" },
    AboutToShow: { inSignature: "i", outSignature: "b" },
    AboutToShowGroup: { inSignature: "ai", outSignature: "aiai" },
  },
  signals: {
    LayoutUpdated: { signature: "ui" },
    ItemsPropertiesUpdated: { signature: "a(ia{sv})a(ias)" },
    ItemActivationRequested: { signature: "iu" },
  },
});

export async function registerWindow(window, language, signal, activate) {
  if (signal.aborted) return;
  const bus = dbus.sessionBus();
  const path = "/com/canonical/menu/" + window;
  const ignore = () => {};
  bus.on("error", ignore);
  let registrar;
  let registered;
  let menu;
  const tellRegistrar = (owner, member, signature, body) =>
    bus.send(
      new Message({
        destination: owner,
        path: REGISTRAR_PATH,
        interface: REGISTRAR,
        member,
        signature,
        body,
        flags: dbus.MessageFlag.NO_REPLY_EXPECTED,
      }),
    );
  signal.addEventListener(
    "abort",
    () => {
      try {
        if (registered) tellRegistrar(registered, "UnregisterWindow", "u", [window]);
      } catch {
        // The session bus can disappear before the window closes.
      } finally {
        bus.disconnect();
      }
    },
    { once: true },
  );
  const register = async (owner) => {
    if (owner === registrar || signal.aborted) return;
    registrar = owner;
    registered = undefined;
    if (!owner) return;
    if (!menu) {
      await selectLanguage(language, [Intl.DateTimeFormat().resolvedOptions().locale]);
      if (signal.aborted || owner !== registrar) return;
      if (!menu) {
        menu = new ApplicationMenu((action) => {
          if (!signal.aborted) activate(action);
        });
        bus.export(path, menu);
      }
    }
    tellRegistrar(owner, "RegisterWindow", "uo", [window, path]);
    registered = owner;
  };
  const daemon = (member, body) =>
    bus.call(
      new Message({
        destination: DBUS,
        path: "/org/freedesktop/DBus",
        interface: DBUS,
        member,
        signature: "s",
        body: [body],
      }),
    );
  let changed = false;
  bus.on("message", (message) => {
    if (
      message.type === dbus.MessageType.SIGNAL &&
      message.sender === DBUS &&
      message.interface === DBUS &&
      message.member === "NameOwnerChanged" &&
      message.body[0] === REGISTRAR
    ) {
      changed = true;
      void register(message.body[2]).catch(ignore);
    }
  });
  await daemon(
    "AddMatch",
    `type='signal',sender='${DBUS}',interface='${DBUS}',member='NameOwnerChanged',arg0='${REGISTRAR}'`,
  );
  const owner = await daemon("GetNameOwner", REGISTRAR).catch(() => null);
  if (!changed && owner) await register(owner.body[0]);
}
