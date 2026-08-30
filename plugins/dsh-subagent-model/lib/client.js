window.__ModuleLoader__.load({
	id: "@dsh-external/dsh-subagent-model",
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
		var LIST_ROUTE = "/subagent-model/api/list";
		var SET_ROUTE = "/subagent-model/api/set";
		var RESET_ROUTE = "/subagent-model/api/reset";
		var STYLE_ID = "sm-subagent-model-style";
		var NL = String.fromCharCode(10);

		// ---------------------------------------------------------------------------
		// 纯工具
		// ---------------------------------------------------------------------------
		function el(tag, className, text) {
			var node = document.createElement(tag);
			if (className) node.className = className;
			if (text !== undefined) node.textContent = text;
			return node;
		}
		function opt(value, label) {
			var o = document.createElement("option");
			o.value = value;
			o.textContent = label;
			return o;
		}
		async function postJson(route, body) {
			var res = await fetch(route, {
				method: "POST",
				headers: { "content-type": "application/json" },
				body: JSON.stringify(body || {}),
			});
			return await res.json();
		}
		async function getJson(route) {
			var res = await fetch(route);
			return await res.json();
		}

		// ---------------------------------------------------------------------------
		// 样式（深色主题，表格布局沿用 plugin-manager 修正版：容器横向滚动 + 最小宽度 + nowrap）
		// ---------------------------------------------------------------------------
		function adoptStyle() {
			if (document.getElementById(STYLE_ID)) return;
			var style = document.createElement("style");
			style.id = STYLE_ID;
			style.textContent = [
				".sm-root{display:flex;flex-direction:column;gap:12px;padding:6px 2px 24px;font-size:13px;color:var(--dsw-alias-content-primary,#eee);max-width:1100px}",
				".sm-hint{padding:8px 12px;border-radius:8px;font-size:12px;background:var(--dsw-alias-state-warn-tertiary,#3a2f16);color:var(--dsw-alias-state-warn-primary,#ffd679);border:1px solid #ffd67944}",
				".sm-conflict{padding:8px 12px;border-radius:8px;font-size:12px;background:var(--dsw-alias-state-error-tertiary,#3d1d1d);color:var(--dsw-alias-state-error-primary,#ff9b9b);border:1px solid #ff9b9b55}",
				".sm-toolbar{display:flex;align-items:center;gap:10px;flex-wrap:wrap}",
				".sm-title{font-weight:600}",
				".sm-sub{color:var(--dsw-alias-content-secondary,#9b9b9b);font-size:12px}",
				".sm-btn{border:1px solid var(--dsw-alias-stroke-default,#3a3a3a);background:transparent;color:inherit;border-radius:8px;padding:6px 14px;cursor:pointer;font-size:12px}",
				".sm-btn:hover{background:var(--dsw-alias-surface-hover,#ffffff12)}",
				".sm-btn:disabled{opacity:.5;cursor:default}",
				".sm-toast{display:none;padding:8px 12px;border-radius:8px;font-size:12px}",
				".sm-toast[data-show='true']{display:block}",
				".sm-toast[data-kind='ok']{background:var(--dsw-alias-state-success-tertiary,#1d3a2a);color:var(--dsw-alias-state-success-primary,#7ee2a8)}",
				".sm-toast[data-kind='err']{background:var(--dsw-alias-state-error-tertiary,#3d1d1d);color:var(--dsw-alias-state-error-primary,#ff9b9b)}",
				".sm-table{border:1px solid var(--dsw-alias-stroke-default,#3a3a3a);border-radius:12px;overflow-x:auto}",
				".sm-table table{width:100%;min-width:680px;border-collapse:collapse;font-size:12px}",
				".sm-table th{text-align:left;padding:8px 12px;background:var(--dsw-alias-surface-sunken,#161617);border-bottom:1px solid var(--dsw-alias-stroke-default,#3a3a3a);font-weight:600;color:var(--dsw-alias-content-secondary,#9b9b9b);white-space:nowrap}",
				".sm-table td{padding:7px 12px;border-bottom:1px solid var(--dsw-alias-stroke-default,#242425);vertical-align:middle}",
				".sm-table tr:last-child td{border-bottom:none}",
				".sm-table tr:hover td{background:#ffffff08}",
				".sm-name{font-weight:600;white-space:nowrap}",
				".sm-mode{border:1px solid #3a3a3a;border-radius:5px;padding:1px 7px;font-size:11px;white-space:nowrap;color:var(--dsw-alias-content-secondary,#9b9b9b)}",
				".sm-eff-managed{color:var(--dsw-alias-state-success-primary,#7ee2a8);white-space:nowrap}",
				".sm-eff-inherit{color:var(--dsw-alias-content-secondary,#9b9b9b);white-space:nowrap}",
				".sm-eff-sub{color:var(--dsw-alias-content-secondary,#9b9b9b);font-size:11px;white-space:nowrap}",
				".sm-select{background:var(--dsw-alias-surface-sunken,#161617);border:1px solid var(--dsw-alias-stroke-default,#3a3a3a);color:inherit;border-radius:6px;padding:4px 8px;font-size:12px;max-width:200px}",
				".sm-select:disabled{opacity:.5}",
				".sm-ops{display:flex;gap:6px;align-items:center;justify-content:flex-end;flex-wrap:nowrap}",
				".sm-act{border:1px solid var(--dsw-alias-stroke-default,#3a3a3a);background:#ffffff0d;color:inherit;border-radius:6px;padding:3px 10px;font-size:11px;cursor:pointer;white-space:nowrap;flex-shrink:0}",
				".sm-act:hover{background:#ffffff1a}",
				".sm-act:disabled{opacity:.45;cursor:default}",
				".sm-act-primary{color:var(--dsw-alias-state-info-primary,#4c8dff);border-color:#4c8dff66}",
				".sm-act-danger{color:var(--dsw-alias-state-error-primary,#ff9b9b);border-color:#ff9b9b66}",
				".sm-note{color:var(--dsw-alias-content-secondary,#9b9b9b);font-size:11px;line-height:1.6}"
			].join(NL);
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

		function findProvider(catalog, name) {
			var list = (catalog && catalog.providers) || [];
			for (var i = 0; i < list.length; i++) { if (list[i].name === name) return list[i]; }
			return null;
		}
		function findModel(provider, id) {
			var list = (provider && provider.models) || [];
			for (var i = 0; i < list.length; i++) { if (list[i].id === id) return list[i]; }
			return null;
		}

		/** 重建模型下拉：随提供商联动；保留 keep（当前生效值不在目录时追加"目录外"项）。 */
		function rebuildModelSel(row, keep) {
			var sel = row.modelSel;
			sel.textContent = "";
			sel.appendChild(opt("", "— 选择模型 —"));
			var prov = findProvider(row.st.data.catalog, row.provSel.value);
			var models = (prov && prov.models) || [];
			for (var i = 0; i < models.length; i++) {
				var o = opt(models[i].id, models[i].id);
				if (models[i].name && models[i].name !== models[i].id) o.title = models[i].name;
				sel.appendChild(o);
			}
			if (keep && !findModel(prov, keep)) sel.appendChild(opt(keep, keep + "（目录外）"));
			sel.value = keep || "";
			sel.disabled = row.provSel.value === "";
		}

		/** 重建思考强度下拉：选项取自该模型 reasoningEfforts 的键；附"跟随继承"项。 */
		function rebuildEffortSel(row, keep) {
			var sel = row.effortSel;
			sel.textContent = "";
			sel.appendChild(opt("", "跟随继承（不写入）"));
			var prov = findProvider(row.st.data.catalog, row.provSel.value);
			var model = findModel(prov, row.modelSel.value);
			var efforts = (model && model.efforts) || [];
			for (var i = 0; i < efforts.length; i++) sel.appendChild(opt(efforts[i], efforts[i]));
			if (keep && efforts.indexOf(keep) < 0) sel.appendChild(opt(keep, keep + "（目录外）"));
			sel.value = keep || "";
			sel.disabled = row.modelSel.value === "";
			if (model && efforts.length === 0) sel.title = "该模型在 settings.yaml 中未声明 reasoningEfforts";
		}

		function syncSaveState(row) {
			row.saveBtn.disabled = row.provSel.value === "" || row.modelSel.value === "";
		}

		function effectiveCell(target) {
			var td = document.createElement("td");
			if (target.managed && target.effective) {
				td.appendChild(el("div", "sm-eff-managed", target.effective.provider + " / " + target.effective.model));
				var extras = [];
				if (target.effective.reasoningEffort) extras.push("effort: " + target.effective.reasoningEffort);
				if (target.effective.maxTokens) extras.push("maxTokens: " + target.effective.maxTokens);
				if (extras.length > 0) td.appendChild(el("div", "sm-eff-sub", extras.join(" · ")));
			} else {
				td.appendChild(el("div", "sm-eff-inherit", "继承主会话"));
			}
			return td;
		}

		function renderRows(st, data) {
			st.tbody.textContent = "";
			st.rows = [];
			(data.targets || []).forEach(function (target) {
				var tr = document.createElement("tr");
				var row = { st: st, target: target };

				var tdName = document.createElement("td");
				tdName.appendChild(el("div", "sm-name", target.toolName));
				var mode = el("div", "sm-eff-sub", target.id + " · " + (target.mode === "continuable" ? "continuable" : "one-shot"));
				tdName.appendChild(mode);

				var tdEff = effectiveCell(target);

				var tdProv = document.createElement("td");
				row.provSel = el("select", "sm-select");
				row.provSel.appendChild(opt("", "— 选择提供商 —"));
				var provs = (data.catalog && data.catalog.providers) || [];
				for (var i = 0; i < provs.length; i++) row.provSel.appendChild(opt(provs[i].name, provs[i].name));
				var curProv = target.effective ? target.effective.provider : "";
				if (curProv && !findProvider(data.catalog, curProv)) row.provSel.appendChild(opt(curProv, curProv + "（目录外）"));
				row.provSel.value = curProv;
				tdProv.appendChild(row.provSel);

				var tdModel = document.createElement("td");
				row.modelSel = el("select", "sm-select");
				tdModel.appendChild(row.modelSel);

				var tdEffort = document.createElement("td");
				row.effortSel = el("select", "sm-select");
				row.effortSel.title = "内核 schema 未声明、实测透传生效的字段";
				tdEffort.appendChild(row.effortSel);

				var tdOps = document.createElement("td");
				var ops = el("div", "sm-ops");
				row.saveBtn = el("button", "sm-act sm-act-primary", "保存");
				row.saveBtn.type = "button";
				row.resetBtn = el("button", "sm-act sm-act-danger", "恢复继承");
				row.resetBtn.type = "button";
				row.resetBtn.disabled = !target.managed;
				ops.append(row.saveBtn, row.resetBtn);
				tdOps.appendChild(ops);

				rebuildModelSel(row, target.effective ? target.effective.model : "");
				rebuildEffortSel(row, target.effective && target.effective.reasoningEffort ? target.effective.reasoningEffort : "");
				syncSaveState(row);

				row.provSel.addEventListener("change", function () {
					rebuildModelSel(row, "");
					rebuildEffortSel(row, "");
					syncSaveState(row);
				});
				row.modelSel.addEventListener("change", function () {
					rebuildEffortSel(row, "");
					syncSaveState(row);
				});
				row.effortSel.addEventListener("change", function () { syncSaveState(row); });
				row.saveBtn.addEventListener("click", function () { void doSave(st, row); });
				row.resetBtn.addEventListener("click", function () { void doReset(st, row); });

				tr.append(tdName, tdEff, tdProv, tdModel, tdEffort, tdOps);
				st.tbody.appendChild(tr);
				st.rows.push(row);
			});
		}

		async function doSave(st, row) {
			var body = { id: row.target.id, provider: row.provSel.value, model: row.modelSel.value };
			if (row.effortSel.value !== "") body.reasoningEffort = row.effortSel.value;
			row.saveBtn.disabled = true;
			try {
				var resp = await postJson(SET_ROUTE, body);
				toast(st, resp.ok ? "✅ " + resp.message : "❌ " + (resp.message || "保存失败"), resp.ok ? "ok" : "err");
			} catch (e) {
				toast(st, "请求失败：" + String(e && e.message ? e.message : e), "err");
			}
			await refreshAll(st);
		}

		async function doReset(st, row) {
			var sure = window.confirm("将「" + row.target.toolName + "」恢复为继承主会话模型？" + NL + "（从 cordis.patch.yml 托管块移除该条目；重启 DSH 后生效。）");
			if (!sure) return;
			row.resetBtn.disabled = true;
			try {
				var resp = await postJson(RESET_ROUTE, { id: row.target.id });
				toast(st, resp.ok ? "✅ " + resp.message : "❌ " + (resp.message || "恢复失败"), resp.ok ? "ok" : "err");
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
				var resp = await getJson(LIST_ROUTE);
				if (resp && resp.ok === true) {
					st.data = resp;
					if (st.fileLabel) st.fileLabel.textContent = resp.patchFile || "";
					var warn = [];
					if (resp.blockBroken) warn.push("⚠️ " + resp.blockBroken);
					if (resp.conflicts && resp.conflicts.length > 0) {
						warn.push("⚠️ 托管块之外已存在 " + resp.conflicts.map(function (c) { return c.id + "（第 " + c.line + " 行）"; }).join("、") + " 条目：保存会被拒绝。请先手工删除或迁移该条目，再用本页管理。");
					}
					st.conflictBox.textContent = warn.join(NL);
					st.conflictBox.style.display = warn.length > 0 ? "block" : "none";
					var noCat = !resp.catalog || !resp.catalog.providers || resp.catalog.providers.length === 0;
					if (noCat) toast(st, "模型目录为空：未在 settings.yaml 找到 llm-pi-ai.providers", "err");
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
			var root = el("div", "sm-root");
			var hint = el("div", "sm-hint", "⚠️ 设置写入 profiles/web/cordis.patch.yml 的托管块，重启 DSH 后生效；写前自动备份（.bak-sm-*，保留最近 10 份）；块外内容字节级不变。");
			root.appendChild(hint);
			var toolbar = el("div", "sm-toolbar");
			var title = el("span", "sm-title", "子代理模型");
			var fileLabel = el("span", "sm-sub", "");
			var refreshBtn = el("button", "sm-btn", "刷新");
			refreshBtn.type = "button";
			toolbar.append(title, fileLabel, refreshBtn);
			root.appendChild(toolbar);
			var conflictBox = el("div", "sm-conflict");
			conflictBox.style.display = "none";
			conflictBox.style.whiteSpace = "pre-line";
			root.appendChild(conflictBox);
			var toastEl = el("div", "sm-toast");
			toastEl.dataset.show = "false";
			root.appendChild(toastEl);
			var table = el("div", "sm-table");
			var t = document.createElement("table");
			var thead = document.createElement("thead");
			var hr = document.createElement("tr");
			["目标", "当前生效", "提供商", "模型", "思考强度", "操作"].forEach(function (label) {
				var th = document.createElement("th");
				th.textContent = label;
				hr.appendChild(th);
			});
			thead.appendChild(hr);
			var tbody = document.createElement("tbody");
			t.append(thead, tbody);
			table.appendChild(t);
			root.appendChild(table);
			var note = el("div", "sm-note", "说明：「思考强度」（reasoningEffort）是内核 agentOptions schema 未声明、但实测被原样透传并生效的字段（schemastery 保留未声明键）；官方若收紧校验，该项可能失效。模型目录只读自 settings.yaml 的 llm-pi-ai.providers；maxTokens 可经 API 设置，本页不提供输入。");
			root.appendChild(note);
			host.appendChild(root);

			var st = {
				root: root, toast: toastEl, toastTimer: null, refreshBtn: refreshBtn,
				tbody: tbody, fileLabel: fileLabel, conflictBox: conflictBox,
				data: null, rows: [], disposed: false,
			};
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
		function SubagentModelSection() {
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
		var name = "@dsh-external/dsh-subagent-model";
		var inject = ["slots"];

		function apply(ctx) {
			adoptStyle();
			ctx.effect(function () {
				return function () {
					var style = document.getElementById(STYLE_ID);
					if (style) style.remove();
				};
			}, "subagent-model: style");
			ctx.slots.inject("settings.section", function () {
				return ctx.slots.register({
					name: "settings.section",
					id: "subagent-model",
					order: 71,
					label: function () { return "子代理模型"; }
				}, SubagentModelSection);
			});
		}

		exports.name = name;
		exports.inject = inject;
		exports.apply = apply;
		return module.exports;
	}
});
