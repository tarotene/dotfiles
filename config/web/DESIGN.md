---
name: Web の面のデザインシステム(機体のマーキング)
description: 鋼色の灰色と墨色の地に、安全黄の枠が「いま」を示し、決めたことが検査札として残る。
colors:
  panel-grey: "#e9ecee"
  sheet-white: "#ffffff"
  ink: "#1b1f23"
  steel: "#5b6670"
  steel-light: "#c3c9ce"
  safety-yellow: "#f2c200"
  go-green: "#197a4b"
  go-wash: "#e6f5ec"
  fault-red: "#ce0000"
  fault-wash: "#fdeeee"
  caution-orange: "#ac3e00"
  caution-wash: "#ffeee2"
typography:
  headline:
    fontFamily: "Noto Sans JP, Noto Sans CJK JP, Hiragino Sans, Yu Gothic, system-ui, sans-serif"
    fontSize: "2rem"
    fontWeight: 700
    lineHeight: 1.4
  title:
    fontFamily: "Noto Sans JP, Noto Sans CJK JP, Hiragino Sans, Yu Gothic, system-ui, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 700
    lineHeight: 1.4
  body:
    fontFamily: "Noto Sans JP, Noto Sans CJK JP, Hiragino Sans, Yu Gothic, system-ui, sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.7
  label:
    fontFamily: "Noto Sans JP, Noto Sans CJK JP, Hiragino Sans, Yu Gothic, system-ui, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 800
    letterSpacing: "0.08em"
  readout:
    fontFamily: "Noto Sans Mono, ui-monospace, SFMono-Regular, Menlo, monospace"
    fontSize: "1em"
    fontWeight: 600
    letterSpacing: "0em"
    fontFeature: "tnum"
rounded:
  sm: "2px"
  md: "4px"
  lg: "6px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  xxl: "32px"
components:
  button-main:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.sheet-white}"
    rounded: "{rounded.md}"
    padding: "8px 24px"
    height: "44px"
  button-sub:
    backgroundColor: "{colors.sheet-white}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    padding: "8px 24px"
    height: "44px"
  choice:
    backgroundColor: "{colors.sheet-white}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    padding: "8px 12px"
    height: "44px"
  step-done:
    backgroundColor: "{colors.sheet-white}"
    textColor: "{colors.ink}"
    rounded: "{rounded.sm}"
  step-current:
    backgroundColor: "{colors.sheet-white}"
    textColor: "{colors.ink}"
    rounded: "{rounded.sm}"
---

# Design System: Web の面のデザインシステム(機体のマーキング)

## Overview

**Creative North Star: "The Walkaround"(出発前の機体点検)**

飛ぶ前の機体を 1 周して、点検した場所に札を付けていく。この世界では、決めたことは消えずに札として残り、いまの 1 件だけが黄色の枠で囲まれ、残りは破線で一段下に退く。借りているのは、外板のステンシル文字の太さ・歩行ラインの黄色・検査札の形だけで、画面の構成と操作は標準のまま(世界は、書体の使い方・配色・密度・署名の動き 1 つを貸す)。

地は鋼色の灰色、面は白、文字は墨色。色の主役は安全黄の 1 色で、「いま」の印にしか使わない。状態の色(成功・誤り・注意)は、この世界とは別に、意味を変えずに保つ。暗の設定では、鋼色の暗い地に明るい鋼色の文字、黄色はそのまま。

この世界は、出発点(`variables.css`・`components.css`・見本ページ)の世界であり、全面に通す世界ではない(ADR-0000)。各面は、自分の目的から世界を選び、自分の `DESIGN.md` に書く。この世界を採る面は、区間の pin で複写してよい。

本人が避けたいと明示した見た目(2026-11-26): 事務的で味気ない / AI が作った感じ。(「面ごとに別物に見える」は、共通のテーマをやめた 2026-11-27 に外した。)

**Key Characteristics:**
- 「いま」は黄色の枠、「済み」は札、「まだ」は破線(状態を形と色の両方で伝える)。
- 数字・時刻・符号は等幅の読み出しで、桁が動かない。
- 位置と大きさがぶれない: 同じ行の件は同じ高さ、選択肢は固定の格子。
- 状態の色の意味と WCAG 2.2 AA は、世界によらない床。役割の名前と主色(accent の 4 つ)の上書きは、この出発点を採る面の約束で、採らない面は持たない。

## Colors

鋼色の灰色と墨色が骨格で、安全黄だけが世界の色。

### Primary
- **Ink**(#1b1f23): 文字、主な操作の塗り(`--accent` の既定)、フォーカスの輪。暗では明るい鋼色(#e9ecee)に反転する。

### Secondary
- **Safety Yellow**(#f2c200): 「いまの件」「いまのステップ」の枠だけ(`--marker`)。明るい背景とのコントラストが 3:1 に届かないので、単独では使わず、必ず墨色(`--text`)の輪郭と対にし、その上に文字を載せない。

### Neutral
- **Panel Grey**(#e9ecee): 画面の地(`--bg`)。暗では #1b1f23。
- **Sheet White**(#ffffff): 面・札・選択肢の塗り(`--surface`)。暗では #252b31。
- **Steel**(#5b6670): 弱い文字と境界(`--text-muted`・`--border`)。暗では #9aa4ad。
- **Steel Light**(#c3c9ce): 区切りの細い線(`--border-subtle`)。暗では #3d464e。

### Status(世界とは別。意味を変えない)
- **Go Green**(#197a4b / 背景 #e6f5ec)、**Fault Red**(#ce0000 / #fdeeee)、**Caution Orange**(#ac3e00 / #ffeee2)。DADS の semantic の緑・赤・橙のまま。色だけに頼らず、文でも伝える。

### Named Rules
**The One Marker Rule.** 黄色は「いま」の 1 か所にだけ現れる。装飾に使わない。
**The Outline Pair Rule.** `--marker` を使う規則は、同じ規則の中で `--text` の輪郭を持つ(`web_components` が検査する)。

## Typography

**Display / Body Font:** Noto Sans JP(Noto Sans CJK JP・Hiragino Sans・Yu Gothic・system-ui の順に降りる)。機体の書体だけを使い、第三者の配信元から読み込まない。
**Readout Font:** Noto Sans Mono(ui-monospace・SFMono-Regular・Menlo に降りる)。

**Character:** 本文は静かな標準の書体、短いラベルと符号は太く(800)間を空けて、ステンシル文字のように読ませる。数字だけが等幅で、桁がそろう。

### Hierarchy
- **Headline**(700、2rem、行高 1.4): 1 画面に 1 つの見出し。
- **Title**(700、1.25rem、行高 1.4): 件の見出し・節の見出し。
- **Body**(400、1rem、行高 1.7): 本文。1 行は和文で 40 字ほど(`--measure` 40em)。
- **Label**(800、0.875rem、字間 0.08em): 件のラベル・KPI の名前などの短い文字。
- **Readout**(600、等幅、`tabular-nums`): 数字・時刻・符号・選択肢の記号。

### Named Rules
**The Readout Only Rule.** 等幅の読み出しは、数字・時刻・符号にだけ使う。段落を等幅にしない。

## Layout

骨格は、固定の枠と固定の格子。1 件ずつ進む面は、画面の高さに収め、動くのは本文の領域だけ。選択肢は 3 列 x 2 行の同じ大きさ(狭い幅は縦に積む)で、位置が件ごとに変わらない。戻るは左端、次へは右端(狭い幅は縦に積み、次へが上)。全件のステップの列は、狭い幅で 6 列、広い幅で 11 列。余白は 4・8・12・16・24・32px の 6 段。本文幅は 40em。

## Elevation & Depth

ほぼ平らで、段は色と枠で示す。例外は「いまの件」だけで、黄色の枠のまま 4px 持ち上がり、墨色を 40% 混ぜた柔らかい影(`0 8px 12px -4px`)を落とす。ほかの面に影を使わない。

### Named Rules
**The One Lifted Rule.** 持ち上がるのは「いまの件」だけ。

## Shapes

角は小さく(2px・4px・6px)、ステンシルの札のように角ばらせる。済みの札は、左上の角だけ 6px で、穴(丸)が 1 つ空く。破線は「まだ」、実線は「済み」、太い黄色の枠は「いま」。

## Components

### Buttons
- **Shape:** 4px の角、高さ 44px 以上(押す領域)。
- **Primary:** 墨色の塗り、白の文字、太字(800)、字間 0.08em。
- **Secondary:** 白の塗り、墨色の文字と枠。
- **Focus:** 3px の輪(墨色。暗では明るい鋼色)、2px 離す。

### Choices
- 白の塗り、2px の鋼色の枠、4px の角。選ぶと枠が墨色になり、内側にもう 2px の線が入る。既定の選択を置かない。

### Steps(署名の部品)
- **Done:** 白い札。左上に穴、番号の下に選んだ記号(墨色の四角に白の等幅)。消えずに残る。
- **Current:** 黄色の太い枠(6px)+墨色の細い輪郭、白の塗り、4px 浮く。
- **Pending:** 破線の枠、弱い文字。
- **Due:** 橙の丸と「!」を右上に重ねる。色だけで伝えない。

### Status / Alerts / Empty / Table
- 保存の表示は、成功を確かめてから「保存済み」。警告は、背景色+枠+文の 3 つで伝える。空の状態は、何が無いかと次の一手を 1 つ書く。表は、狭い幅で 1 行 1 枚のカードに組み替わる。

## Do's and Don'ts

### Do:
- **Do** 決めた件は札として残し、消さない。
- **Do** 「いま」の印に黄色を使うときは、墨色の輪郭と対にする(`--marker` + `--text`)。
- **Do** 数字・時刻・符号は等幅の読み出しにそろえる。
- **Do** 同じ行の件は同じ高さに、選択肢は同じ大きさ・同じ位置にする。

### Don't:
- **Don't** 黄色を装飾や大きな塗りに使わない(「工事中」の見た目に寄る)。
- **Don't** 黄色の上に文字を載せない(明暗ともコントラストが足りない)。
- **Don't** 状態の色(成功・誤り・注意)の意味を、この世界の色で変えない。
- **Don't** 第三者の書体・スタイルを読み込まない。段落を等幅にしない。
- **Don't** グラデーション・ガラス・生成りの地と飾り書体にしない(避けたい見た目)。
