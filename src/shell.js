// Ayudas de las páginas locales: invocar al escritorio y leer la URL.
window.shell = {
  invoke: function (cmd, args) {
    var t = window.__TAURI_INTERNALS__;
    if (!t) return Promise.reject({ message: "Escritorio no disponible" });
    return t.invoke(cmd, args || {}).catch(function (e) {
      throw e && typeof e === "object" ? e : { code: "error", message: String(e) };
    });
  },
  param: function (k) { return new URLSearchParams(location.search).get(k); },
  pie: function () {
    window.shell.invoke("shell_state").then(function (s) {
      var el = document.querySelector(".pie");
      if (el) el.textContent = "Alfredo Caja " + s.version + " · " + s.deviceId + (s.origin ? " · " + s.origin.replace("https://", "") : "");
    }).catch(function () {});
  },
};
document.addEventListener("DOMContentLoaded", function () { window.shell.pie(); });
