// Command gen-weapons builds data/weapons.json from the item name table of
// wnc-replay/replay-tool (`analysis/names.go`, MIT). The source file is not vendored:
// download it (not with a .go extension, which `go run` would compile) and pass its path.
//
//	curl -L -o names.txt https://raw.githubusercontent.com/wnc-replay/replay-tool/<commit>/analysis/names.go
//	go run tools/gen-weapons/main.go names.txt data/weapons.json tools/r6-dissect/dissect/game_operators.go
//
// Operator entries go to the Go table (r6-dissect uses them to read operator loadout blocks);
// map entries are skipped. In weapons.json the kind comes from the source section:
// weapon, ability (operator gadget), gadget (secondary gadget), reinforcement, drone,
// objective, equipment or other.
//
// Entries that carry a `"source"` field in the existing weapons.json (names matched by hand or by
// elimination, e.g. `"source": "inferred"`) are not in the name table; they are read back from the
// old file and kept, so regenerating does not lose them. A `source` entry wins over the table.
package main

import (
	"fmt"
	"os"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

type entry struct {
	id     uint64
	name   string
	kind   string
	source string // only set for entries kept from the old weapons.json
}

var pairRe = regexp.MustCompile(`0[xX]([0-9A-Fa-f]+)\s*:\s*"((?:[^"\\]|\\.)*)"`)

// sectionKind maps a section comment of names.go to a kind; "" skips the section.
func sectionKind(comment string, current string) (string, bool) {
	c := strings.ToUpper(strings.TrimSpace(comment))
	switch {
	case c == "MAPS":
		return "", true
	case c == "OPERATORS":
		return "operator", true
	case c == "WEAPONS":
		return "weapon", true
	case strings.HasPrefix(c, "--- WALL REINFORCEMENTS"):
		return "reinforcement", true
	case strings.HasPrefix(c, "--- DRONES"):
		return "drone", true
	case strings.HasPrefix(c, "--- DEFUSER"):
		return "objective", true
	case strings.HasPrefix(c, "--- OPERATOR PRIMARY GADGETS"):
		return "ability", true
	case strings.HasPrefix(c, "--- DRONE WALL"), strings.HasPrefix(c, "--- UNIVERSAL SECONDARY GADGETS"):
		return "gadget", true
	case c == "RAPPEL DEVICE":
		return "equipment", true
	case strings.HasPrefix(c, "OSP "):
		return "gadget", true
	case strings.HasPrefix(c, "--- META BANK"):
		return "other", true
	}
	return current, false
}

var manualRe = regexp.MustCompile(`^\s*"(\d+)":\s*\{\s*"name":\s*("(?:[^"\\]|\\.)*")\s*,\s*"kind":\s*"([a-z]*)"\s*,\s*"source":\s*"([a-z]+)"`)

// mergeManual adds the entries of the old weapons.json that have a "source" field to out,
// replacing generated entries with the same id.
func mergeManual(out []entry, oldPath string) []entry {
	old, err := os.ReadFile(oldPath)
	if err != nil {
		return out
	}
	byID := map[uint64]int{}
	for i, e := range out {
		byID[e.id] = i
	}
	for _, line := range strings.Split(strings.ReplaceAll(string(old), "\r\n", "\n"), "\n") {
		m := manualRe.FindStringSubmatch(line)
		if m == nil {
			continue
		}
		id, err := strconv.ParseUint(m[1], 10, 64)
		name, err2 := strconv.Unquote(m[2])
		if err != nil || err2 != nil {
			continue
		}
		e := entry{id, name, m[3], m[4]}
		if i, ok := byID[id]; ok {
			out[i] = e
		} else {
			out = append(out, e)
		}
	}
	return out
}

func main() {
	if len(os.Args) != 4 {
		fmt.Fprintln(os.Stderr, "usage: gen-weapons <names.go saved as .txt> <weapons.json> <game_operators.go>")
		os.Exit(2)
	}
	src, err := os.ReadFile(os.Args[1])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	kind := ""
	inTable := false
	seen := map[uint64]string{}
	var out, operators []entry
	for _, line := range strings.Split(strings.ReplaceAll(string(src), "\r\n", "\n"), "\n") {
		t := strings.TrimSpace(line)
		if strings.HasPrefix(t, "var gameItemNames") {
			inTable = true
			continue
		}
		if !inTable {
			continue
		}
		if t == "}" {
			break
		}
		if strings.HasPrefix(t, "//") {
			kind, _ = sectionKind(strings.TrimPrefix(t, "//"), kind)
			continue
		}
		if kind == "" {
			continue
		}
		for _, m := range pairRe.FindAllStringSubmatch(t, -1) {
			id, err := strconv.ParseUint(m[1], 16, 64)
			if err != nil {
				fmt.Fprintln(os.Stderr, "bad id:", m[1])
				os.Exit(1)
			}
			name, err := strconv.Unquote(`"` + m[2] + `"`)
			if err != nil {
				fmt.Fprintln(os.Stderr, "bad name:", m[2])
				os.Exit(1)
			}
			if name == "R6GameObjectBank_Content" {
				continue
			}
			if prev, ok := seen[id]; ok {
				fmt.Fprintf(os.Stderr, "duplicate id %d: %q and %q\n", id, prev, name)
				os.Exit(1)
			}
			seen[id] = name
			if kind == "operator" {
				operators = append(operators, entry{id, name, kind, ""})
			} else {
				out = append(out, entry{id, name, kind, ""})
			}
		}
	}
	out = mergeManual(out, os.Args[2])
	sort.Slice(out, func(i, j int) bool { return out[i].id < out[j].id })
	var b strings.Builder
	b.WriteString("{\n")
	for i, e := range out {
		name := strconv.Quote(e.name)
		src := ""
		if e.source != "" {
			src = fmt.Sprintf(", \"source\": %q", e.source)
		}
		fmt.Fprintf(&b, "  \"%d\": { \"name\": %s, \"kind\": %q%s }", e.id, name, e.kind, src)
		if i < len(out)-1 {
			b.WriteString(",")
		}
		b.WriteString("\n")
	}
	b.WriteString("}\n")
	if err := os.WriteFile(os.Args[2], []byte(b.String()), 0o644); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	sort.Slice(operators, func(i, j int) bool { return operators[i].id < operators[j].id })
	var g strings.Builder
	g.WriteString("// Code generated by tools/gen-weapons from wnc-replay/replay-tool analysis/names.go (MIT). DO NOT EDIT.\n\n")
	g.WriteString("package dissect\n\n")
	g.WriteString("// gameOperatorNames maps game item IDs of operators (as in loadout blocks) to names.\n")
	g.WriteString("var gameOperatorNames = map[uint64]string{\n")
	for _, e := range operators {
		fmt.Fprintf(&g, "\t0x%X: %s,\n", e.id, strconv.Quote(e.name))
	}
	g.WriteString("}\n")
	if err := os.WriteFile(os.Args[3], []byte(g.String()), 0o644); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	counts := map[string]int{"operator": len(operators)}
	for _, e := range out {
		counts[e.kind]++
	}
	fmt.Printf("%d items: %v\n", len(out), counts)
}
