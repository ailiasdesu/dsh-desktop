// desktop-health：内核健康探针（壳层心跳，可选），200 → 活
export const name = "desktop-health";
export const inject = ["webServer"];

export function apply(ctx) {
  ctx.effect(
    () =>
      ctx.webServer.register({
        kind: "exact",
        path: "/health",
        handler: async (_req, res) => {
          res.writeHead(200, { "content-type": "text/plain; charset=utf-8" });
          res.end("ok");
        },
      }),
    "desktop-health: route"
  );
}
