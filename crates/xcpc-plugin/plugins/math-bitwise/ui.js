"use strict";

function render(sdk) {
  const { h, signal, computed, ui, token } = sdk;
  const { View, Text, Scroll, Button, Segmented, Input, If, Output } = ui;

  const MASK64 = (1n << 64n) - 1n;
  const SIGN64 = 1n << 63n;

  const TAB_ITEMS = [
    { value: "bits", label: "位运算", testID: "math-tab-bits" },
    { value: "nt", label: "数论计算", testID: "math-tab-nt" },
    { value: "mods", label: "常用模数", testID: "math-tab-mods" },
  ];

  const activeTab = signal("bits"); // "bits" | "nt" | "mods"
  const bitValue = signal(998244353n);
  const rawInput = signal("998244353");
  const parseError = signal("");
  const moreMenuOpen = signal(false);

  // Number theory tab state
  const ntN = signal("998244353");
  const ntA = signal("3");
  const ntB = signal("998244351");
  const ntMod = signal("998244353");
  const ntCrt = signal("2 3; 3 5; 2 7");
  const ntResult = signal(null);
  const ntError = signal("");
  let ntTimer = null;
  let ntSeq = 0;

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
    const seq = ++ntSeq;
    ntError.set("");
    try {
      const res = await sdk.runTool("number-theory", {
        n: ntN(),
        a: ntA(),
        b: ntB(),
        mod: ntMod(),
        crt: ntCrt(),
      });
      if (seq === ntSeq) ntResult.set(res);
    } catch (e) {
      if (seq === ntSeq) ntError.set(String(e && e.message ? e.message : e));
    }
  }

  function scheduleNt() {
    if (ntTimer) clearTimeout(ntTimer);
    ntTimer = setTimeout(() => {
      ntTimer = null;
      computeNumberTheory();
    }, 120);
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
    if (Output) {
      return h(Output, { blocks: res.blocks });
    }
    return h(
      View,
      { style: { gap: 12 } },
      ...res.blocks.map((blk) =>
        h(
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
          h(Text, { style: { fontSize: 12, color: token("foreground") } }, String(blk.content || blk.title || ""))
        )
      )
    );
  }

  function renderBitGroup(g) {
    const hi = g * 8 + 7;
    const lo = g * 8;
    const cells = [];
    for (let b = hi; b >= lo; b--) {
      const bitIndex = b;
      const isOn = () => ((bitValue() >> BigInt(bitIndex)) & 1n) === 1n;
      cells.push(
        h(
          View,
          {
            testID: "math-bit-" + bitIndex,
            onPress: () => flipBit(bitIndex),
            style: () => ({
              flex: 1,
              minWidth: 0,
              height: 36,
              borderRadius: 4,
              borderWidth: 1,
              borderColor: isOn() ? token("primary") : token("border"),
              backgroundColor: isOn() ? token("primary") : token("card"),
              alignItems: "center",
              justifyContent: "center",
              gap: 1,
            }),
          },
          h(
            Text,
            {
              style: () => ({
                fontSize: 12.5,
                fontWeight: "600",
                fontFamily: "JetBrains Mono, monospace",
                fontVariant: ["tabular-nums"],
                color: isOn() ? token("primary-foreground") : token("foreground"),
              }),
            },
            () => (isOn() ? "1" : "0")
          ),
          h(
            Text,
            {
              style: () => ({
                fontSize: 9,
                fontFamily: "JetBrains Mono, monospace",
                fontVariant: ["tabular-nums"],
                color: isOn() ? token("primary-foreground") : token("muted-foreground"),
              }),
            },
            String(bitIndex)
          )
        )
      );
    }
    return h(
      View,
      {
        testID: "math-bit-group-" + g,
        style: {
          flex: 1,
          minWidth: 0,
          padding: 8,
          borderRadius: 8,
          borderWidth: 1,
          borderColor: token("border"),
          backgroundColor: token("muted"),
          gap: 5,
        },
      },
      h(
        Text,
        {
          style: {
            fontSize: 10.5,
            color: token("muted-foreground"),
            fontVariant: ["tabular-nums"],
          },
        },
        "位 " + hi + "–" + lo
      ),
      h(View, { style: { flexDirection: "row", gap: 4 } }, ...cells)
    );
  }

  function renderBitGrid() {
    const rows = [];
    for (let r = 0; r < 4; r++) {
      const leftGroup = 7 - r * 2;
      const rightGroup = 6 - r * 2;
      rows.push(
        h(
          View,
          { style: { flexDirection: "row", gap: 8, width: "100%" } },
          renderBitGroup(leftGroup),
          renderBitGroup(rightGroup)
        )
      );
    }
    return h(View, { testID: "math-bit-grid", style: { gap: 8, width: "100%" } }, ...rows);
  }

  function renderBitsTab() {
    const statRows = [
      ["无符号 u64", () => bitStats().u64],
      ["有符号 i64", () => bitStats().i64],
      ["十六进制", () => bitStats().hex],
      ["八进制", () => bitStats().oct],
      ["1 的个数 / 前导零 / 尾随零", () => bitStats().popcount + " / " + bitStats().clz + " / " + bitStats().ctz],
      ["最低位 1（x & -x）", () => bitStats().lowbit],
    ];
    return h(
      View,
      { testID: "math-panel-bits", style: { gap: 12, position: "relative" } },
      h(
        View,
        { style: { gap: 4 } },
        h(
          View,
          { style: { flexDirection: "row", gap: 8, alignItems: "center" } },
          h(Input, {
            testID: "math-bit-input",
            value: () => rawInput(),
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
            style: { flex: 1, minWidth: 180, fontFamily: "JetBrains Mono, monospace" },
          }),
          h(Button, { testID: "math-bit-clear", variant: "outline", size: "sm", onPress: () => setU64(0n) }, "清零"),
          h(Button, { testID: "math-bit-not", variant: "outline", size: "sm", onPress: () => setU64(~bitValue()) }, "取反 ~x"),
          h(
            Button,
            {
              testID: "math-bit-copy-hex",
              variant: "outline",
              size: "sm",
              onPress: () => {
                const hex = bitStats().hex;
                if (sdk.copyText) sdk.copyText(hex);
                if (sdk.toast) sdk.toast("已复制 " + hex);
              },
            },
            "复制十六进制"
          ),
          h(Button, {
            testID: "math-bit-more",
            variant: "outline",
            size: "sm",
            icon: "ellipsis",
            style: () => ({
              borderColor: moreMenuOpen() ? token("input") : token("border"),
              backgroundColor: moreMenuOpen() ? token("accent") : token("card"),
            }),
            onPress: () => moreMenuOpen.set(!moreMenuOpen()),
          })
        ),
        // Fixed-height slot so showing/clearing a parse error never shifts the 64-bit grid
        h(
          View,
          { style: { height: 18, justifyContent: "center" } },
          h(
            Text,
            { style: { fontSize: 12, lineHeight: 16, color: token("destructive") } },
            () => parseError() || ""
          )
        )
      ),
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
        ...statRows.map(([k, getVal]) =>
          h(
            View,
            { style: { flexDirection: "row", justifyContent: "space-between", gap: 12 } },
            h(Text, { style: { fontSize: 12, color: token("muted-foreground") } }, k),
            h(
              Text,
              {
                style: {
                  fontSize: 12.5,
                  fontFamily: "JetBrains Mono, monospace",
                  fontVariant: ["tabular-nums"],
                  color: token("foreground"),
                },
              },
              getVal
            )
          )
        )
      ),
      // Floating overflow popover + dismiss backdrop (absolute positioned: zero layout shift)
      h(
        If,
        { when: () => moreMenuOpen() },
        () =>
          h(
            View,
            {
              style: {
                position: "absolute",
                top: 0,
                left: 0,
                right: 0,
                bottom: 0,
                zIndex: 10,
              },
            },
            h(View, {
              testID: "math-bit-more-backdrop",
              onPress: () => moreMenuOpen.set(false),
              style: {
                position: "absolute",
                top: 0,
                left: 0,
                right: 0,
                bottom: 0,
              },
            }),
            h(
              View,
              {
                testID: "math-bit-more-menu",
                style: {
                  position: "absolute",
                  top: 36,
                  right: 0,
                  zIndex: 20,
                  flexDirection: "row",
                  alignItems: "center",
                  gap: 4,
                  padding: 6,
                  borderRadius: 8,
                  borderWidth: 1,
                  borderColor: token("border"),
                  backgroundColor: token("popover"),
                  boxShadow: "0 4px 12px rgba(0, 0, 0, 0.12)",
                },
              },
              h(
                Button,
                {
                  testID: "math-bit-all-ones",
                  variant: "ghost",
                  size: "sm",
                  style: { borderRadius: 4 },
                  onPress: () => {
                    setU64(MASK64);
                    moreMenuOpen.set(false);
                  },
                },
                "全 1"
              ),
              h(
                Button,
                {
                  testID: "math-bit-shl",
                  variant: "ghost",
                  size: "sm",
                  style: { borderRadius: 4 },
                  onPress: () => {
                    setU64(bitValue() << 1n);
                    moreMenuOpen.set(false);
                  },
                },
                "<< 1"
              ),
              h(
                Button,
                {
                  testID: "math-bit-shr",
                  variant: "ghost",
                  size: "sm",
                  style: { borderRadius: 4 },
                  onPress: () => {
                    setU64(bitValue() >> 1n);
                    moreMenuOpen.set(false);
                  },
                },
                ">> 1"
              )
            )
          )
      )
    );
  }

  function renderNumberTheoryTab() {
    if (!ntResult()) {
      computeNumberTheory();
    }
    return h(
      View,
      { testID: "math-panel-nt", style: { gap: 12 } },
      h(
        View,
        {
          style: {
            padding: 12,
            borderRadius: 8,
            borderWidth: 1,
            borderColor: token("border"),
            backgroundColor: token("card"),
            gap: 10,
          },
        },
        h(
          View,
          { style: { flexDirection: "row", flexWrap: "wrap", gap: 8 } },
          h(
            View,
            { style: { flex: 1, minWidth: 150, gap: 4 } },
            h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "整数 n（分解 / φ(n)）"),
            h(Input, {
              value: () => ntN(),
              onChangeText: (v) => { ntN.set(v); scheduleNt(); },
              style: { fontFamily: "JetBrains Mono, monospace" },
            })
          ),
          h(
            View,
            { style: { flex: 1, minWidth: 110, gap: 4 } },
            h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "参数 a"),
            h(Input, {
              value: () => ntA(),
              onChangeText: (v) => { ntA.set(v); scheduleNt(); },
              style: { fontFamily: "JetBrains Mono, monospace" },
            })
          ),
          h(
            View,
            { style: { flex: 1, minWidth: 110, gap: 4 } },
            h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "参数 b"),
            h(Input, {
              value: () => ntB(),
              onChangeText: (v) => { ntB.set(v); scheduleNt(); },
              style: { fontFamily: "JetBrains Mono, monospace" },
            })
          ),
          h(
            View,
            { style: { flex: 1, minWidth: 130, gap: 4 } },
            h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "模数 m"),
            h(Input, {
              value: () => ntMod(),
              onChangeText: (v) => { ntMod.set(v); scheduleNt(); },
              style: { fontFamily: "JetBrains Mono, monospace" },
            })
          )
        ),
        h(
          View,
          { style: { gap: 4 } },
          h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "同余方程组（分号分隔，每组 r m）"),
          h(Input, {
            value: () => ntCrt(),
            onChangeText: (v) => { ntCrt.set(v); scheduleNt(); },
            style: { fontFamily: "JetBrains Mono, monospace" },
          })
        )
      ),
      h(
        If,
        { when: () => ntError() },
        () => h(Text, { style: { fontSize: 12, color: token("destructive") } }, () => ntError())
      ),
      h(
        If,
        { when: () => ntResult() },
        (res) => renderBlocks(res)
      )
    );
  }

  function renderModTab() {
    if (!modResult()) {
      loadModReference();
    }
    return h(
      View,
      { testID: "math-panel-mods", style: { gap: 12 } },
      h(
        If,
        { when: () => modResult() },
        (res) => renderBlocks(res)
      )
    );
  }

  return h(
    View,
    { style: { flex: 1, padding: 16, gap: 12, minHeight: 0, backgroundColor: token("background") } },
    h(
      View,
      { style: { flexDirection: "row", alignItems: "center" } },
      h(Segmented, {
        items: TAB_ITEMS,
        value: () => activeTab(),
        itemWidth: 86,
        onChange: (tab) => {
          moreMenuOpen.set(false);
          activeTab.set(tab);
          if (tab === "nt" && !ntResult()) computeNumberTheory();
          if (tab === "mods" && !modResult()) loadModReference();
        },
      })
    ),
    h(
      Scroll,
      { style: { flex: 1, minHeight: 0 } },
      h(If, { when: () => activeTab() === "bits" }, () => renderBitsTab()),
      h(If, { when: () => activeTab() === "nt" }, () => renderNumberTheoryTab()),
      h(If, { when: () => activeTab() === "mods" }, () => renderModTab())
    )
  );
}

module.exports = { render };
