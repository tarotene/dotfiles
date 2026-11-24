/*
 * 見本の動く部分: behavior.js の使い方の例(ADR-1703 D2)。インラインの `<script>` を置けない見本が、同一オリジンで読み込む。
 * 失敗は 1 回目だけ起こす(再保存・再読み込みの操作で、成功に戻れることを見せる)。
 */
(function () {
  "use strict";

  var b = window.pageBehavior;
  var byId = function (id) {
    return document.getElementById(id);
  };
  var later = function (ms, fn) {
    return new Promise(function (resolve, reject) {
      setTimeout(function () {
        try {
          resolve(fn());
        } catch (err) {
          reject(err);
        }
      }, ms);
    });
  };
  var two = function (n) {
    return ("0" + n).slice(-2);
  };

  // 保存の表示
  var failNextSave = false;
  var save = b.saver(
    byId("demo-save-status"),
    function () {
      return later(400, function () {
        if (failNextSave) {
          failNextSave = false;
          throw new Error("offline");
        }
        return true;
      });
    },
    {
      labels: {
        saved: function (d) {
          return "保存済み " + two(d.getHours()) + ":" + two(d.getMinutes());
        },
      },
    }
  );
  byId("demo-save-ok").addEventListener("click", function () {
    failNextSave = false;
    save.run("ok");
  });
  byId("demo-save-fail").addEventListener("click", function () {
    failNextSave = true;
    save.run("fail");
  });

  // ショートカットのキー: ボタンにフォーカスがあるとき、Enter・Space は、ショートカットでなくボタンを押す
  var out = byId("demo-key-out");
  var say = function (name) {
    return function () {
      out.textContent = name + " のショートカットが呼ばれた";
    };
  };
  b.keyguard({ 1: say("1"), " ": say("Space"), Enter: say("Enter") });
  var clicks = 0;
  byId("demo-key-button").addEventListener("click", function () {
    clicks += 1;
    byId("demo-key-clicks").textContent = "押された回数: " + clicks;
  });

  // 読み込み中・失敗・空の出し分け
  var view = b.viewstate(byId("demo-view"), {
    labels: { empty: "まだ項目がありません。上のボタンで読み込みます。" },
  });
  view.empty();
  var failNextLoad = false;
  var render = function (items, content) {
    var ul = document.createElement("ul");
    items.forEach(function (text) {
      var li = document.createElement("li");
      li.textContent = text;
      ul.appendChild(li);
    });
    content.appendChild(ul);
  };
  var load = function (items) {
    return view.run(function () {
      return later(300, function () {
        if (failNextLoad) {
          failNextLoad = false;
          throw new Error("offline");
        }
        return items;
      });
    }, render);
  };
  byId("demo-load-ok").addEventListener("click", function () {
    failNextLoad = false;
    load(["更新手続きの確認", "フィルタの反映"]);
  });
  byId("demo-load-empty").addEventListener("click", function () {
    failNextLoad = false;
    load([]);
  });
  byId("demo-load-fail").addEventListener("click", function () {
    failNextLoad = true;
    load(["更新手続きの確認", "フィルタの反映"]);
  });
})();
