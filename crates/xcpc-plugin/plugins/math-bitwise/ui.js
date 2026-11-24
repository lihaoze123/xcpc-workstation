"use strict";

function render(sdk) {
  const { h, signal, computed, ui, token } = sdk;
  const { View, Text, Scroll, Button, Input, If } = ui;

  const MASK64 = (1n << 64n) - 1n;
  const SIGN64 = 1n << 63n;

  const activeTab = signal("bits"); // "bits" | "nt" | "mods"
  const bitValue = signal(998244353n);
  const rawInput = signal("998244353");
  const parseError = signal("");

  // Number theory tab state
  const ntN = signal("998244353");
  const ntA = signal("3");
  const ntB = signal("998244351");
  const ntMod = signal("998244353");
  const ntCrt = signal("2 3; 3 5; 2 7");
  const ntResult = signal(null);
  const ntError = signal("");
  const ntLoading = signal(false);

  // Mod reference state
  const modResult = signal(null);

  function parseBig(raw) {
    const s = String(raw || "0").trim().replace(/_/g, "");
    if (!s) return 0n;
    if (/^[+-]?0[xX][0-9a-fA-F]+$/.test(s)) {
      return s.startsWith("-") ? -BigInt(s.slice(1)) : BigInt(s.replace(/^\+/, ""));
    }
    if (/^[+-]?0[bB][01]+$/.test(s)) {
      return s.startsWith("-") ? -BigInt(s.slice(1)) : BigInt(s.replace(/^\+/, ""));
    }
    if (/^[+-]?0[oO][0-7]+$/.test(s)) {
      return s.startsWith("-") ? -BigInt(s.slice(1)) : BigInt(s.replace(/^\+/, ""));
    }
    if (/^[+-]?\d+$/.test(s)) return BigInt(s);
    throw new Error("无效整数格式");
  }

  function setU64(v) {
    const u = v & MASK64;
    bitValue.set(u);
    rawInput.set(u.toString());
    parseError.set("");
  }

  function flipBit(bitIdx) {
    const mask = 1n << BigInt(bitIdx);
    setU64(bitValue() ^ mask);
  }

  const bitStats = computed(() => {
    const u = bitValue();
    const signed = (u & SIGN64) !== 0n ? u - (1n << 64n) : u;
    let pop = 0;
    let tmp = u;
    while (tmp > 0n) {
      tmp &= tmp - 1n;
      pop++;
    }
    const ctz = u === 0n ? 64 : (() => {
      let c = 0;
      let x = u;
      while ((x & 1n) === 0n) { x >>= 1n; c++; }
      return c;
    })();
    const clz = u === 0n ? 64 : 64 - u.toString(2).length;
    const lowbit = u === 0n ? 0n : u & ((-u) & MASK64);
    return {
      u64: u.toString(),
      i64: signed.toString(),
      hex: "0x" + u.toString(16).toUpperCase().padStart(16, "0"),
      oct: "0o" + u.toString(8),
      bin: u.toString(2).padStart(64, "0"),
      popcount: pop,
      clz,
      ctz,
      lowbit: lowbit.toString(),
    };
  });

  async function computeNumberTheory() {
    ntLoading.set(true);
    ntError.set("");
    try {
      const res = await sdk.runTool("number-theory", {
        n: ntN(),
        a: ntA(),
        b: ntB(),
        mod: ntMod(),
        crt: ntCrt(),
      });
      ntResult.set(res);
    } catch (e) {
      ntError.set(String(e && e.message ? e.message : e));
    } finally {
      ntLoading.set(false);
    }
  }

  async function loadModReference() {
    if (modResult()) return;
    try {
      const res = await sdk.runTool("mod-reference", {});
      modResult.set(res);
    } catch (_) {
      // ignore
    }
  }

  function renderBlocks(res) {
    if (!res || !Array.isArray(res.blocks)) return null;
    return h(
      View,
      { style: { gap: 12 } },
      ...res.blocks.map((blk) => {
        if (blk.type === "table") {
          return h(
            View,
            {
              style: {
                borderWidth: 1,
                borderColor: token("border"),
                borderRadius: 8,
                overflow: "hidden",
                backgroundColor: token("card"),
              },
            },
            blk.title
              ? h(
                  View,
                  {
                    style: {
                      paddingHorizontal: 12,
                      paddingVertical: 8,
                      borderBottomWidth: 1,
                      borderColor: token("border"),
                      backgroundColor: token("muted"),
                    },
                  },
                  h(Text, { style: { fontSize: 12.5, fontWeight: "600", color: token("foreground") } }, blk.title)
                )
              : null,
            h(
              View,
              {
                style: {
                  flexDirection: "row",
                  paddingHorizontal: 12,
                  paddingVertical: 6,
                  borderBottomWidth: 1,
                  borderColor: token("border"),
                },
              },
              ...(blk.headers || []).map((col) =>
                h(
                  Text,
                  {
                    style: {
                      flex: 1,
                      fontSize: 11.5,
                      fontWeight: "600",
                      color: token("muted-foreground"),
                    },
                  },
                  col
                )
              )
            ),
            ...(blk.rows || []).map((row, idx) =>
              h(
                View,
                {
                  style: {
                    flexDirection: "row",
                    paddingHorizontal: 12,
                    paddingVertical: 6,
                    borderBottomWidth: idx + 1 < blk.rows.length ? 1 : 0,
                    borderColor: token("border"),
                  },
                },
                ...row.map((cell) =>
                  h(
                    Text,
                    {
                      style: {
                        flex: 1,
                        fontSize: 12,
                        fontFamily: "JetBrains Mono, monospace",
                        color: token("foreground"),
                      },
                    },
                    String(cell)
                  )
                )
              )
            )
          );
        }
        return h(
          View,
          {
            style: {
              padding: 10,
              borderRadius: 8,
              borderWidth: 1,
              borderColor: token("border"),
              backgroundColor: token("card"),
            },
          },
          h(Text, { style: { fontSize: 12, color: token("foreground") } }, String(blk.content || ""))
        );
      })
    );
  }

  function renderBitGrid() {
    const u = bitValue();
    const groups = [];
    for (let g = 7; g >= 0; g--) {
      const hi = g * 8 + 7;
      const lo = g * 8;
      const cells = [];
      for (let b = hi; b >= lo; b--) {
        const isOn = ((u >> BigInt(b)) & 1n) === 1n;
        cells.push(
          h(
            View,
            {
              onPress: () => flipBit(b),
              style: {
                width: 30,
                height: 36,
                borderRadius: 6,
                borderWidth: 1,
                borderColor: isOn ? token("primary") : token("border"),
                backgroundColor: isOn ? token("primary") : token("card"),
                alignItems: "center",
                justifyContent: "center",
                gap: 1,
              },
            },
            h(
              Text,
              {
                style: {
                  fontSize: 12.5,
                  fontWeight: "600",
                  fontFamily: "JetBrains Mono, monospace",
                  color: isOn ? token("primary-foreground") : token("foreground"),
                },
              },
              isOn ? "1" : "0"
            ),
            h(
              Text,
              {
                style: {
                  fontSize: 9,
                  fontFamily: "JetBrains Mono, monospace",
                  color: isOn ? token("primary-foreground") : token("muted-foreground"),
                },
              },
              String(b)
            )
          )
        );
      }
      groups.push(
        h(
          View,
          {
            style: {
              padding: 6,
              borderRadius: 8,
              borderWidth: 1,
              borderColor: token("border"),
              backgroundColor: token("muted"),
              gap: 4,
            },
          },
          h(
            Text,
            { style: { fontSize: 10, color: token("muted-foreground"), fontFamily: "JetBrains Mono, monospace" } },
            "bits " + hi + ".." + lo
          ),
          h(View, { style: { flexDirection: "row", gap: 4 } }, ...cells)
        )
      );
    }
    return h(View, { style: { flexDirection: "row", flexWrap: "wrap", gap: 8 } }, ...groups);
  }

  function renderBitsTab() {
    const st = bitStats();
    return h(
      View,
      { style: { gap: 12 } },
      h(
        View,
        { style: { flexDirection: "row", gap: 8, alignItems: "center", flexWrap: "wrap" } },
        h(Input, {
          value: rawInput(),
          placeholder: "输入十进制 / 0x十六进制 / 0b二进制 / 负数",
          onChangeText: (txt) => {
            rawInput.set(txt);
            try {
              const parsed = parseBig(txt);
              bitValue.set(parsed & MASK64);
              parseError.set("");
            } catch (e) {
              parseError.set(String(e && e.message ? e.message : e));
            }
          },
          style: { flex: 1, minWidth: 220 },
        }),
        h(Button, { variant: "outline", size: "sm", onPress: () => setU64(0n) }, "清零"),
        h(Button, { variant: "outline", size: "sm", onPress: () => setU64(MASK64) }, "全 1"),
        h(Button, { variant: "outline", size: "sm", onPress: () => setU64(~bitValue()) }, "取反 ~x"),
        h(Button, { variant: "outline", size: "sm", onPress: () => setU64(bitValue() << 1n) }, "<< 1"),
        h(Button, { variant: "outline", size: "sm", onPress: () => setU64(bitValue() >> 1n) }, ">> 1"),
        h(
          Button,
          {
            variant: "outline",
            size: "sm",
            onPress: () => {
              if (sdk.copyText) sdk.copyText(st.hex);
              if (sdk.toast) sdk.toast("已复制 " + st.hex);
            },
          },
          "复制 Hex"
        )
      ),
      parseError()
        ? h(Text, { style: { fontSize: 12, color: token("destructive") } }, parseError())
        : null,
      renderBitGrid(),
      h(
        View,
        {
          style: {
            borderWidth: 1,
            borderColor: token("border"),
            borderRadius: 8,
            padding: 12,
            backgroundColor: token("card"),
            gap: 6,
          },
        },
        ...[
          ["无符号 u64", st.u64],
          ["有符号 i64", st.i64],
          ["十六进制", st.hex],
          ["八进制", st.oct],
          ["popcount / clz / ctz", st.popcount + " / " + st.clz + " / " + st.ctz],
          ["lowbit (x & -x)", st.lowbit],
        ].map(([k, v]) =>
          h(
            View,
            { style: { flexDirection: "row", justifyContent: "space-between", gap: 12 } },
            h(Text, { style: { fontSize: 12, color: token("muted-foreground") } }, k),
            h(
              Text,
              { style: { fontSize: 12.5, fontFamily: "JetBrains Mono, monospace", color: token("foreground") } },
              v
            )
          )
        )
      )
    );
  }

  function renderNumberTheoryTab() {
    return h(
      View,
      { style: { gap: 12 } },
      h(
        View,
        { style: { flexDirection: "row", flexWrap: "wrap", gap: 8 } },
        h(
          View,
          { style: { flex: 1, minWidth: 160, gap: 4 } },
          h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "整数 n（分解 / φ(n)）"),
          h(Input, { value: ntN(), onChangeText: (v) => ntN.set(v) })
        ),
        h(
          View,
          { style: { flex: 1, minWidth: 120, gap: 4 } },
          h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "参数 a"),
          h(Input, { value: ntA(), onChangeText: (v) => ntA.set(v) })
        ),
        h(
          View,
          { style: { flex: 1, minWidth: 120, gap: 4 } },
          h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "参数 b"),
          h(Input, { value: ntB(), onChangeText: (v) => ntB.set(v) })
        ),
        h(
          View,
          { style: { flex: 1, minWidth: 140, gap: 4 } },
          h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "模数 m"),
          h(Input, { value: ntMod(), onChangeText: (v) => ntMod.set(v) })
        )
      ),
      h(
        View,
        { style: { gap: 4 } },
        h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "CRT 方程组（分号分隔，每组 r m）"),
        h(
          View,
          { style: { flexDirection: "row", gap: 8 } },
          h(Input, { value: ntCrt(), onChangeText: (v) => ntCrt.set(v), style: { flex: 1 } }),
          h(Button, { onPress: () => computeNumberTheory(), disabled: ntLoading() }, "计算")
        )
      ),
      ntError()
        ? h(Text, { style: { fontSize: 12, color: token("destructive") } }, ntError())
        : null,
      renderBlocks(ntResult())
    );
  }

  function renderModTab() {
    if (!modResult()) {
      loadModReference();
    }
    return renderBlocks(modResult());
  }

  return h(
    View,
    { style: { flex: 1, padding: 16, gap: 12, backgroundColor: token("background") } },
    h(
      View,
      { style: { flexDirection: "row", gap: 8, alignItems: "center" } },
      h(
        Button,
        {
          variant: activeTab() === "bits" ? "default" : "outline",
          size: "sm",
          onPress: () => activeTab.set("bits"),
        },
        "64 位按位翻转"
      ),
      h(
        Button,
        {
          variant: activeTab() === "nt" ? "default" : "outline",
          size: "sm",
          onPress: () => {
            activeTab.set("nt");
            if (!ntResult()) computeNumberTheory();
          },
        },
        "数论计算"
      ),
      h(
        Button,
        {
          variant: activeTab() === "mods" ? "default" : "outline",
          size: "sm",
          onPress: () => {
            activeTab.set("mods");
            loadModReference();
          },
        },
        "常用模数"
      )
    ),
    h(
      Scroll,
      { style: { flex: 1 } },
      h(If, { when: () => activeTab() === "bits" }, () => renderBitsTab()),
      h(If, { when: () => activeTab() === "nt" }, () => renderNumberTheoryTab()),
      h(If, { when: () => activeTab() === "mods" }, () => renderModTab())
    )
  );
}

module.exports = { render };
