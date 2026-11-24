// 撮影用の疑似の保存先(window.claude の db)。合成データだけで、実際の保存先には触れない。
// 状態の撮影定義(visual.toml)の globals で挙動を選ぶ: mode = "ok"(既定。書き込みが成功する) | "fail"(書き込みが拒否される)。
(function () {
  var mode = (window.__VISUAL__ || {}).mode || "ok";
  function listen(payload) {
    return function (ok) {
      setTimeout(function () { ok(payload); }, 0);
      return function () {};
    };
  }
  var emptyDoc = { exists: false, data: function () { return null; } };
  function doc() {
    return {
      onSnapshot: listen(emptyDoc),
      set: function () { return mode === "fail" ? Promise.reject({ code: "unavailable" }) : Promise.resolve(); },
    };
  }
  var db = {
    collection: function () { return { doc: doc, onSnapshot: listen({ docs: [] }) }; },
    doc: doc,
  };
  window.claude = { use: function () { return Promise.resolve(db); } };
})();
