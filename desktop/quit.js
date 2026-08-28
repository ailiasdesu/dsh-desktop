// desktop-quit：内核优雅退出路由（Windows 下唯一优雅路径，research-core §4.2 实测契约）
// 挂载方式：--patch desktop.patch.yml（模块名必须是 file:/// URL）
export const name = "desktop-quit";
export const inject = ["webServer"];

export function apply(ctx) {
  ctx.effect(
    () =>
      ctx.webServer.register({
        kind: "exact",
        path: "/quit",
        handler: async (_req, res) => {
          res.writeHead(200, { "content-type": "text/plain; charset=utf-8" });
          res.end("bye");
          setImmediate(() => {
            const exit = ctx.get("appExit");
            if (exit) exit(0);
          });
        },
      }),
    "desktop-quit: route"
  );
}
