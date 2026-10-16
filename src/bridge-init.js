// Alfredo Caja de escritorio · script de inicialización (F9 T9.3).
//
// Corre en cada carga de la ventana principal ANTES que el JS de la página. Arma
// `window.alfredoDesktop` SÓLO si la página es la web staff del local
// (https://*.app.asistentealfredo.cl o el alias .app.restaurantya.cl). Rust vuelve a
// exigirlo (capability `remote` + chequeo de la URL en cada comando): una página externa
// abierta a la fuerza no tiene puente (RF9.3). Contrato tipado: @repo/types `AlfredoDesktop`.
(function () {
  "use strict";
  if (window.__alfredoDesktopInstalled) return;
  window.__alfredoDesktopInstalled = true;

  var SUFFIXES = [".app.asistentealfredo.cl", ".app.restaurantya.cl"];
  var BRIDGE_VERSION = 1;
  var host = String(location.hostname || "").toLowerCase();
  var allowed =
    location.protocol === "https:" &&
    SUFFIXES.some(function (s) { return host.length > s.length && host.slice(-s.length) === s; });

  // ── Eventos desde Rust (window.__alfredoDesktopEmit, vía eval) ────────────────
  var listeners = {};
  window.__alfredoDesktopEmit = function (event, payload) {
    var list = (listeners[event] || []).slice();
    list.forEach(function (cb) { try { cb(payload); } catch (e) { console.error(e); } });
    if (event === "tray-action" && payload && payload.action === "quit" && list.length === 0) {
      if (window.confirm("¿Salir? Dejarás de recibir avisos de pedidos en este equipo.")) invoke("shell_quit");
    }
  };

  function invoke(cmd, args) {
    var t = window.__TAURI_INTERNALS__;
    if (!t || typeof t.invoke !== "function") return Promise.reject({ code: "error", message: "Puente no disponible" });
    return t.invoke(cmd, args || {}).catch(function (e) {
      if (e && typeof e === "object" && e.code) throw { code: e.code, message: e.message || String(e.code) };
      throw { code: "error", message: typeof e === "string" ? e : (e && e.message) || "Error del escritorio" };
    });
  }

  function toBase64(bytes) {
    var bin = "";
    var CH = 0x8000;
    for (var i = 0; i < bytes.length; i += CH) {
      bin += String.fromCharCode.apply(null, Array.prototype.slice.call(bytes.subarray(i, i + CH)));
    }
    return btoa(bin);
  }

  // Kiosco: sin menú contextual ni herramientas; Ctrl+Shift+K pide salir (la web pide el PIN).
  window.addEventListener("contextmenu", function (e) { if (window.__alfredoKiosk) e.preventDefault(); }, true);
  window.addEventListener("keydown", function (e) {
    var k = (e.key || "").toLowerCase();
    if (window.__alfredoKiosk && (k === "f12" || (e.ctrlKey && e.shiftKey && (k === "i" || k === "j")))) {
      e.preventDefault();
      e.stopPropagation();
    }
    if (e.ctrlKey && e.shiftKey && k === "k") {
      e.preventDefault();
      window.__alfredoDesktopEmit("kiosk-exit-request", {});
    }
  }, true);

  if (!allowed) return;

  // Sesión de la persona del turno recuperada por el escritorio (T9.2): la web la consume
  // UNA vez por sesión de la ventana (marca en sessionStorage), así cerrar sesión y recargar
  // no la revive.
  var raw = window.__ALFREDO_DESKTOP_BOOT_RAW__;
  try { delete window.__ALFREDO_DESKTOP_BOOT_RAW__; } catch (e) { window.__ALFREDO_DESKTOP_BOOT_RAW__ = undefined; }
  window.__ALFREDO_DESKTOP_BOOT__ = raw && raw.personToken ? { personToken: raw.personToken, personUntil: raw.personUntil || null } : null;

  var fire = function (p) { p.catch(function (e) { console.warn("[alfredoDesktop]", e && e.message); }); };

  var desktop = {
    bridgeVersion: BRIDGE_VERSION,
    info: function () { return invoke("desktop_info"); },
    printers: {
      list: function () { return invoke("printers_list"); },
      save: function (cfg) { return invoke("printers_save", { cfg: cfg }); },
      remove: function (key) { return invoke("printers_remove", { key: key }); },
      discover: function (opts) { return invoke("printers_discover", { network: !opts || opts.network !== false }); },
      print: function (key, bytes, opts) {
        var u8 = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes || []);
        return invoke("printers_print", { key: key, data: toBase64(u8), copies: opts && opts.copies ? opts.copies : null });
      },
      status: function (key) { return invoke("printers_status", { key: key }); },
      // Sólo Mac (F10): cola de CUPS para una térmica USB; macOS pide la clave de administrador.
      createQueue: function (model, deviceUri) { return invoke("printers_create_queue", { model: model, deviceUri: deviceUri }); }
    },
    notify: {
      incomingOrders: function (count, text) { fire(invoke("notify_incoming", { count: Math.max(0, count | 0), text: text || null })); },
      clear: function () { fire(invoke("notify_clear")); }
    },
    window: {
      bringToFront: function () { fire(invoke("window_bring_to_front")); },
      setKiosk: function (on) { return invoke("window_set_kiosk", { on: !!on }); }
    },
    power: {
      keepAwake: function (on, display) { return invoke("power_keep_awake", { on: !!on, display: display !== false }); }
    },
    update: {
      check: function () { return invoke("update_check"); },
      install: function () { return invoke("update_install"); }
    },
    support: {
      startRemote: function () { return invoke("support_start_remote"); },
      copyDiagnostics: function (extra) { return invoke("support_copy_diagnostics", { extra: extra || null }); }
    },
    session: {
      rememberPerson: function (token, until) { return invoke("session_remember", { token: token || null, until: until || null }); },
      recallPerson: function () { return invoke("session_recall"); }
    },
    settings: {
      setAutostart: function (on) { return invoke("settings_set_autostart", { on: !!on }); }
    },
    on: function (event, cb) {
      (listeners[event] = listeners[event] || []).push(cb);
      return function () { listeners[event] = (listeners[event] || []).filter(function (x) { return x !== cb; }); };
    }
  };
  try {
    Object.defineProperty(window, "alfredoDesktop", { value: Object.freeze(desktop), writable: false, configurable: false });
  } catch (e) {
    window.alfredoDesktop = desktop;
  }

  function ready() { fire(invoke("page_ready")); }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", ready, { once: true });
  else ready();
})();
