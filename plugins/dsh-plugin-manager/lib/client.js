window.__ModuleLoader__.load({
	id: "@dsh-external/dsh-plugin-manager",
	factory: (require) => {
		var module = { exports: {} };
		var exports = module.exports;
		var react = require("react");
		var h = react.createElement;
		var useEffect = react.useEffect;
		var useRef = react.useRef;

		// ---------------------------------------------------------------------------
		// 常量
		// ---------------------------------------------------------------------------
		var LIST_ROUTE = "/plugin-manager/api/list";
		var TOGGLE_ROUTE = "/plugin-manager/api/toggle";
		var ADD_ROUTE = "/plugin-manager/api/add";
		var REMOVE_ROUTE = "/plugin-manager/api/remove";
		var STYLE_ID = "pm-plugin-manager-style";
		var BS = String.fromCharCode(92);

		// ---------------------------------------------------------------------------
		// 纯工具
		// ---------------------------------------------------------------------------
		function el(tag, className, text) {
			var node = document.createElement(tag);
			if (className) node.className = className;
			if (text !== undefined) node.textContent = text;
			return node;
		}
		function shortPath(p) {
			if (!p) return "（不可解析）";
			var s = String(p).split(BS).join("/");
			return s.length > 60 ? s.slice(0, 32) + " … " + s.slice(-24) : s;
		}
		function desc(meta) {
			return (meta && (meta.description || meta.version)) ? ((meta.description || "") + (meta.version ? " v" + meta.version : "")) : "";
		}
		function emptyMsg(text) {
			return el("div", "pm-empty", text);
		}
		async function postJson(route, body) {
			var res = await fetch(route, {
				method: "POST",
				headers: { "content-type": "application/json" },
				body: JSON.stringify(body || {}),
			});
			return await res.json();
		}

		// ---------------------------------------------------------------------------
		// 样式（深色主题，贴近官方 web 样式变量）
		// ---------------------------------------------------------------------------
		function adoptStyle() {
			if (document.getElementById(STYLE_ID)) return;
			var style = document.createElement("style");
			style.id = STYLE_ID;
			style.textContent = [
				".pm-root{display:flex;flex-direction:column;gap:12px;padding:6px 2px;font-size:13px;color:var(--dsw-alias-content-primary,#eee);max-width:1100px}",
				".pm-hint{padding:8px 12px;border-radius:8px;font-size:12px;background:var(--dsw-alias-state-warn-tertiary,#3a2f16);color:var(--dsw-alias-state-warn-primary,#ffd679);border:1px solid #ffd67944}",
				".pm-toolbar{display:flex;align-items:center;gap:10px;flex-wrap:wrap}",
				".pm-title{font-weight:600}",
				".pm-sub{color:var(--dsw-alias-content-secondary,#9b9b9b);font-size:12px}",
				".pm-input{flex:1;min-width:220px;background:var(--dsw-alias-surface-sunken,#161617);border:1px solid var(--dsw-alias-stroke-default,#3a3a3a);color:inherit;border-radius:8px;padding:6px 10px;font-size:12px}",
				".pm-btn{border:1px solid var(--dsw-alias-stroke-default,#3a3a3a);background:transparent;color:inherit;border-radius:8px;padding:6px 14px;cursor:pointer;font-size:12px}",
				".pm-btn:hover{background:var(--dsw-alias-surface-hover,#ffffff12)}",
				".pm-btn:disabled{opacity:.5;cursor:default}",
				".pm-btn-primary{border-color:var(--dsw-alias-state-info-primary,#4c8dff);color:var(--dsw-alias-state-info-primary,#4c8dff)}",
				".pm-toast{display:none;padding:8px 12px;border-radius:8px;font-size:12px}",
				".pm-toast[data-show='true']{display:block}",
				".pm-toast[data-kind='ok']{background:var(--dsw-alias-state-success-tertiary,#1d3a2a);color:var(--dsw-alias-state-success-primary,#7ee2a8)}",
				".pm-toast[data-kind='err']{background:var(--dsw-alias-state-error-tertiary,#3d1d1d);color:var(--dsw-alias-state-error-primary,#ff9b9b)}",
				".pm-table{border:1px solid var(--dsw-alias-stroke-default,#3a3a3a);border-radius:12px;overflow:hidden}",
				".pm-table table{width:100%;border-collapse:collapse;font-size:12px}",
				".pm-table th{text-align:left;padding:8px 12px;background:var(--dsw-alias-surface-sunken,#161617);border-bottom:1px solid var(--dsw-alias-stroke-default,#3a3a3a);font-weight:600;color:var(--dsw-alias-content-secondary,#9b9b9b)}",
				".pm-table td{padding:7px 12px;border-bottom:1px solid var(--dsw-alias-stroke-default,#242425);vertical-align:middle}",
				".pm-table tr:last-child td{border-bottom:none}",
				".pm-table tr:hover td{background:#ffffff08}",
				".pm-kind{border:1px solid #3a3a3a;border-radius:5px;padding:1px 7px;font-size:11px;white-space:nowrap}",
				".pm-kind-bundle{color:var(--dsw-alias-state-success-primary,#7ee2a8);border-color:#7ee2a855}",
				".pm-kind-installed{color:var(--dsw-alias-state-info-primary,#4c8dff);border-color:#4c8dff55}",
				".pm-kind-patch{color:var(--dsw-alias-state-warn-primary,#ffd679);border-color:#ffd67944}",
				".pm-name{font-weight:600;white-space:nowrap}",
				".pm-path{color:var(--dsw-alias-content-secondary,#9b9b9b);font-size:11px;word-break:break-all}",
				".pm-exists-ok{color:var(--dsw-alias-state-success-primary,#7ee2a8)}",
				".pm-exists-no{color:var(--dsw-alias-state-error-primary,#ff9b9b)}",
				".pm-ops{display:flex;gap:6px;align-items:center;justify-content:flex-end}",
				".pm-act{border:1px solid var(--dsw-alias-stroke-default,#3a3a3a);background:#ffffff0d;color:inherit;border-radius:6px;padding:3px 10px;font-size:11px;cursor:pointer}",
				".pm-act:hover{background:#ffffff1a}",
				".pm-act-danger{color:var(--dsw-alias-state-error-primary,#ff9b9b);border-color:#ff9b9b66}",
				".pm-act[data-on='true']{background:var(--dsw-alias-state-success-primary,#1d3a2a);color:#7ee2a8}",
				".pm-empty{color:var(--dsw-alias-content-secondary,#9b9b9b);padding:16px;text-align:center;font-size:12px}",
				".pm-ro{color:var(--dsw-alias-content-secondary,#9b9b9b);font-size:11px}"
			].join(String.fromCharCode(10));
			document.head.appendChild(style);
		}

		// ---------------------------------------------------------------------------
		// 视图逻辑
		// ---------------------------------------------------------------------------
		function toast(st, text, kind) {
			if (!st || st.disposed) return;
			st.toast.textContent = text;
			st.toast.dataset.kind = kind;
			st.toast.dataset.show = "true";
			if (st.toastTimer !== null) window.clearTimeout(st.toastTimer);
			st.toastTimer = window.setTimeout(function () {
				st.toast.dataset.show = "false";
				st.toastTimer = null;
			}, 4500);
		}

		function kindBadge(kind) {
			var label = kind === "bundle" ? "bundle 启用" : (kind === "installed" ? "已安装未启用" : "patch 只读");
			var b = el("span", "pm-kind pm-kind-" + kind, label);
			return b;
		}

		function exitBadge(exists) {
			return el("span", exists ? "pm-exists-ok" : "pm-exists-no", exists ? "✓ 已解析" : "✗ 未找到");
		}

		function renderRows(st, data) {
			st.tbody.textContent = "";
			var all = [];
			(data.bundles || []).forEach(function (b) { all.push(b); });
			(data.installed || []).forEach(function (b) { all.push(b); });
			(data.patches || []).forEach(function (b) { all.push(b); });
			if (all.length === 0) {
				var row = document.createElement("tr");
				var td = document.createElement("td");
				td.colSpan = 5;
				td.appendChild(emptyMsg("暂无插件数据"));
				row.appendChild(td);
				st.tbody.appendChild(row);
				return;
			}
			all.forEach(function (item) {
				var tr = document.createElement("tr");
				var tdName = document.createElement("td");
				var nm = el("div", "pm-name", item.name);
				var ds = desc(item.meta);
				if (ds) nm.title = ds;
				tdName.appendChild(nm);
				if (ds) tdName.appendChild(el("div", "pm-sub", ds));
				var tdKind = document.createElement("td");
				tdKind.appendChild(kindBadge(item.kind));
				var tdPath = document.createElement("td");
				tdPath.appendChild(el("div", "pm-path", shortPath(item.path || (item.kind === "patch" ? item.patchFile : ""))));
				if (item.kind === "patch" && item.id && item.id !== item.name) tdPath.appendChild(el("div", "pm-ro", "row id: " + item.id));
				var tdExists = document.createElement("td");
				tdExists.appendChild(exitBadge(item.exists));
				var tdOps = document.createElement("td");
				var ops = el("div", "pm-ops");
				if (item.kind === "bundle") {
					ops.appendChild(actBtn("停用", function () { void doToggle(st, item.name, false); }, false, true));
					ops.appendChild(actBtn("移除", function () { void doRemove(st, item.name); }, true, false));
				} else if (item.kind === "installed") {
					ops.appendChild(actBtn("启用", function () { void doToggle(st, item.name, true); }, false, false));
				} else {
					ops.appendChild(el("span", "pm-ro", "只读"));
				}
				tdOps.appendChild(ops);
				tr.append(tdName, tdKind, tdPath, tdExists, tdOps);
				st.tbody.appendChild(tr);
			});
		}

		function actBtn(label, onClick, danger, onState) {
			var b = el("button", "pm-act" + (danger ? " pm-act-danger" : ""), label);
			b.type = "button";
			if (onState) b.dataset.on = "true";
			b.addEventListener("click", onClick);
			return b;
		}

		async function doToggle(st, name, enabled) {
			st.refreshBtn.disabled = true;
			try {
				var resp = await postJson(TOGGLE_ROUTE, { name: name, enabled: enabled });
				toast(st, resp.ok ? "✅ " + resp.message : "❌ " + (resp.message || "操作失败"), resp.ok ? "ok" : "err");
			} catch (e) {
				toast(st, "请求失败：" + String(e && e.message ? e.message : e), "err");
			}
			await refreshAll(st);
		}

		async function doRemove(st, name) {
			var sure = window.confirm("⚠️ 将「" + name + "」从 profile bundles 中移除？" + String.fromCharCode(10) + "（此操作仅编辑 bundles 列表，不删除插件目录；重启 DSH 后生效。可在下方按路径重新添加。）");
			if (!sure) return;
			st.refreshBtn.disabled = true;
			try {
				var resp = await postJson(REMOVE_ROUTE, { name: name });
				toast(st, resp.ok ? "✅ " + resp.message : "❌ " + (resp.message || "移除失败"), resp.ok ? "ok" : "err");
			} catch (e) {
				toast(st, "请求失败：" + String(e && e.message ? e.message : e), "err");
			}
			await refreshAll(st);
		}

		async function doAdd(st, p) {
			p = String(p || "").trim();
			if (p === "") { toast(st, "请输入插件目录的绝对路径", "err"); return; }
			st.refreshBtn.disabled = true;
			try {
				var resp = await postJson(ADD_ROUTE, { path: p });
				toast(st, resp.ok ? "✅ " + resp.message : "❌ " + (resp.message || "添加失败"), resp.ok ? "ok" : "err");
				if (resp.ok) st.addInput.value = "";
			} catch (e) {
				toast(st, "请求失败：" + String(e && e.message ? e.message : e), "err");
			}
			await refreshAll(st);
		}

		async function refreshAll(st) {
			if (!st || st.disposed) return;
			st.refreshBtn.disabled = true;
			st.refreshBtn.textContent = "刷新中…";
			try {
				var resp = await postJson(LIST_ROUTE, {});
				if (resp && resp.ok === true) {
					st.data = resp;
					if (st.profileLabel) st.profileLabel.textContent = (resp.profile ? resp.profile.dir : "") + "";
					renderRows(st, resp);
				} else {
					toast(st, "列表接口失败：" + (resp && resp.message ? resp.message : "未知错误"), "err");
				}
			} catch (e) {
				toast(st, "列表接口异常：" + String(e && e.message ? e.message : e), "err");
			}
			st.refreshBtn.disabled = false;
			st.refreshBtn.textContent = "刷新";
		}

		// ---------------------------------------------------------------------------
		// 挂载（设置面板 section）
		// ---------------------------------------------------------------------------
		function mount(host) {
			var root = el("div", "pm-root");
			var hint = el("div", "pm-hint", "⚠️ 插件列表的改动在重启 DSH 后生效；所有写操作自动备份 profile package.json（.bak-pm-*，保留最近 10 份）。");
			root.appendChild(hint);
			var toolbar = el("div", "pm-toolbar");
			var title = el("span", "pm-title", "插件管理");
			var profileLabel = el("span", "pm-sub", "");
			toolbar.append(title, profileLabel);
			var addInput = el("input", "pm-input");
			addInput.type = "text";
			addInput.placeholder = "输入插件目录绝对路径（含 lib/index.js）";
			var addBtn = el("button", "pm-btn pm-btn-primary", "添加");
			addBtn.type = "button";
			var refreshBtn = el("button", "pm-btn", "刷新");
			refreshBtn.type = "button";
			toolbar.append(addInput, addBtn, refreshBtn);
			root.appendChild(toolbar);
			var toastEl = el("div", "pm-toast");
			toastEl.dataset.show = "false";
			root.appendChild(toastEl);
			var table = el("div", "pm-table");
			var t = document.createElement("table");
			var thead = document.createElement("thead");
			var hr = document.createElement("tr");
			["名称", "类型", "路径", "解析状态", "操作"].forEach(function (h) {
				var th = document.createElement("th");
				th.textContent = h;
				hr.appendChild(th);
			});
			thead.appendChild(hr);
			var tbody = document.createElement("tbody");
			t.append(thead, tbody);
			table.appendChild(t);
			root.appendChild(table);
			host.appendChild(root);

			var st = {
				root: root, toast: toastEl, toastTimer: null, refreshBtn: refreshBtn,
				tbody: tbody, profileLabel: profileLabel, data: null, disposed: false,
				addInput: addInput,
			};
			addBtn.addEventListener("click", function () { void doAdd(st, addInput.value); });
			addInput.addEventListener("keydown", function (ev) { if (ev.key === "Enter") void doAdd(st, addInput.value); });
			refreshBtn.addEventListener("click", function () { void refreshAll(st); });
			void refreshAll(st);
			return st;
		}

		function disposeUI(st) {
			if (!st || st.disposed) return;
			st.disposed = true;
			if (st.toastTimer !== null) window.clearTimeout(st.toastTimer);
		}

		// ---------------------------------------------------------------------------
		// React 壳（挂进设置面板 section；内容是纯 DOM）
		// ---------------------------------------------------------------------------
		function PluginManagerSection() {
			var ref = useRef(null);
			useEffect(function () {
				var host = ref.current;
				if (!host) return;
				var st = mount(host);
				return function () {
					disposeUI(st);
					st.root.remove();
				};
			}, []);
			return h("div", { ref: ref, style: { width: "100%" } });
		}

		// ---------------------------------------------------------------------------
		// 插件契约
		// ---------------------------------------------------------------------------
		var name = "@dsh-external/dsh-plugin-manager";
		var inject = ["slots"];

		function apply(ctx) {
			adoptStyle();
			ctx.effect(function () {
				return function () {
					var style = document.getElementById(STYLE_ID);
					if (style) style.remove();
				};
			}, "plugin-manager: style");
			ctx.slots.inject("settings.section", function () {
				return ctx.slots.register({
					name: "settings.section",
					id: "plugin-manager",
					order: 70,
					label: function () { return "插件管理"; }
				}, PluginManagerSection);
			});
		}

		exports.name = name;
		exports.inject = inject;
		exports.apply = apply;
		return module.exports;
	}
});
