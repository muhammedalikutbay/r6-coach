package dissect

// Kill weapon slots from player inventories.
//
// Adapted (R6Koc patch 5) from prestonwillis10/r6-dissect `dissect/loadout.go` (MIT,
// commit 98e6a680bd322ba322689dde4b192b6946167705). Differences from that file:
//   - no preparation-phase gate (this fork has no round clock); instead every asset ever seen
//     in a slot is remembered, so inventories emptied at the round end or re-sent after an
//     operator swap do not erase what a player carried;
//   - no `Loadout` in the JSON output; only the slot of each kill weapon is emitted, plus the
//     game item ID of that slot (loadout_items.go).

import (
	"bytes"
	"encoding/binary"
)

// WeaponSlot is the inventory slot that holds a kill weapon.
type WeaponSlot string

const (
	PrimarySlot   WeaponSlot = "primary"
	SecondarySlot WeaponSlot = "secondary"
	GadgetSlot    WeaponSlot = "gadget"
	AbilitySlot   WeaponSlot = "ability"
)

// Inventory layout (verified upstream on Y8S1, Y8S2, Y9S1 and Y11S2):
//
// Every player object owns an inventory object:
//
//	1B <player obj> E8D1E539 <inventory> 579FB57B
//
// The inventory references one item per slot, either as an edge
// (1B <inventory> <slot field> <item> <class>) or, when the inventory is
// still the current context (after a chain of empty slots), as a
// reference (1A <slot field> <item> <class>). Empty slots have a zero item
// handle and class. A newly created item is followed by its body, which
// holds the item's asset ID:
//
//	23 <item> 3BD73FC1 01 xx
//	1A D19C5716 <sub> 7882DE67
//	1B <sub> A57AAA29 <asset obj> <class>
//	23 <asset obj> 0E9EBE88 08 <asset id>
//
// The player object also precedes each player's username packet:
//
//	1B <player obj> 27C08DCA ... 22 07949BDC <username>
var (
	inventoryOwnerField = []byte{0xE8, 0xD1, 0xE5, 0x39}
	inventoryClass      = []byte{0x57, 0x9F, 0xB5, 0x7B}
	playerObjectField   = []byte{0x27, 0xC0, 0x8D, 0xCA}
	inventorySlotFields = [inventorySlots][]byte{
		{0xED, 0xE5, 0xAC, 0xBC}, // 0: drone (attack)
		{0x93, 0x27, 0xA1, 0x2B}, // 1: primary
		{0x03, 0x99, 0x40, 0x83}, // 2: secondary
		{0x4C, 0xD6, 0xA0, 0xC7}, // 3: unique ability
		{0xD8, 0x90, 0xB5, 0xF7}, // 4: secondary gadget
	}
	itemBodyMarkers = []struct {
		offset  int
		pattern []byte
	}{
		{0, []byte{0x23}},
		{9, []byte{0x3B, 0xD7, 0x3F, 0xC1, 0x01}},
		{15, []byte{0x1A, 0xD1, 0x9C, 0x57, 0x16}},
		{28, []byte{0x78, 0x82, 0xDE, 0x67, 0x1B}},
		{41, []byte{0xA5, 0x7A, 0xAA, 0x29}},
		{57, []byte{0x23}},
		{66, []byte{0x0E, 0x9E, 0xBE, 0x88, 0x08}},
	}
	// slot lookup order; the first slot holding an asset wins
	slotOrder = []struct {
		index int
		slot  WeaponSlot
	}{
		{1, PrimarySlot},
		{2, SecondarySlot},
		{4, GadgetSlot},
		{3, AbilitySlot},
	}
)

const (
	inventorySlots    = 5
	handleSize        = 8
	slotTokenSize     = 4 + handleSize + 4 // field, item handle, class
	itemBodySize      = 79
	playerObjectRange = 96 // max distance from the player object field to the username packet
)

type inventoryPlayer struct {
	dissectID []byte
	username  string
}

type inventoryState struct {
	owners   map[uint64]uint64                    // inventory handle -> player object handle
	items    map[uint64]*[inventorySlots][]uint64 // inventory handle -> item handles seen per slot
	order    []uint64                             // inventory handles in first-seen order
	assets   map[uint64]uint64                    // item handle -> asset ID
	players  map[uint64]inventoryPlayer           // player object handle -> player
	refOwner uint64                               // inventory a reference-form slot token may belong to
	refNext  int                                  // offset at which that reference-form token must start
	// operator + slot hash -> game item IDs from the operator loadout blocks (loadout_items.go)
	operatorItems map[operatorSlot][]uint64
}

func newInventoryState() *inventoryState {
	return &inventoryState{
		owners:  make(map[uint64]uint64),
		items:   make(map[uint64]*[inventorySlots][]uint64),
		assets:  make(map[uint64]uint64),
		players: make(map[uint64]inventoryPlayer),
		refNext: -1,

		operatorItems: make(map[operatorSlot][]uint64),
	}
}

func (r *Reader) listenLoadouts() {
	if r.Header.CodeVersion < Y8S1 {
		return
	}
	r.inventory = newInventoryState()
	r.Listen(inventoryOwnerField, readInventoryOwner)
	r.Listen(operatorBlockField, readOperatorBlock)
	for i := range inventorySlotFields {
		slot := i
		r.Listen(inventorySlotFields[i], func(r *Reader) error {
			readInventorySlot(r, slot)
			return nil
		})
	}
}

func readHandle(b []byte) uint64 {
	return binary.LittleEndian.Uint64(b)
}

// patternStart returns the offset of the pattern a listener was called for.
// Listeners run with r.offset one byte past the last pattern byte.
func patternStart(r *Reader, pattern []byte) int {
	return r.offset - len(pattern)
}

// readInventoryOwner reads 1B <player obj> E8D1E539 <inventory> 579FB57B.
// It only peeks at r.b and never fails.
func readInventoryOwner(r *Reader) error {
	inv := r.inventory
	start := patternStart(r, inventoryOwnerField)
	if inv == nil || start < handleSize+1 || start+slotTokenSize > len(r.b) || r.b[start-handleSize-1] != 0x1B {
		return nil
	}
	if !bytes.Equal(r.b[start+4+handleSize:start+slotTokenSize], inventoryClass) {
		return nil
	}
	owner := readHandle(r.b[start-handleSize : start])
	handle := readHandle(r.b[start+4 : start+4+handleSize])
	if owner == 0 || handle == 0 {
		return nil
	}
	inv.owners[handle] = owner
	inv.refOwner = handle
	inv.refNext = -1
	return nil
}

func readInventorySlot(r *Reader, slot int) {
	inv := r.inventory
	start := patternStart(r, inventorySlotFields[slot])
	if inv == nil || start < 1 || start+slotTokenSize > len(r.b) {
		return
	}
	var handle uint64
	switch {
	case start >= handleSize+1 && r.b[start-handleSize-1] == 0x1B:
		handle = readHandle(r.b[start-handleSize : start])
		if _, ok := inv.owners[handle]; !ok {
			return
		}
	case r.b[start-1] == 0x1A && start-1 == inv.refNext && inv.refOwner != 0:
		handle = inv.refOwner
	default:
		return
	}
	item := readHandle(r.b[start+4 : start+4+handleSize])
	end := start + slotTokenSize
	if item == 0 {
		if !bytes.Equal(r.b[start+4+handleSize:end], []byte{0, 0, 0, 0}) {
			return
		}
		// an empty slot keeps the inventory as the current context
		inv.refOwner = handle
		inv.refNext = end
		return
	}
	inv.refNext = -1
	if asset, ok := readItemBody(r.b, end, item); ok {
		inv.assets[item] = asset
	}
	items, ok := inv.items[handle]
	if !ok {
		items = &[inventorySlots][]uint64{}
		inv.items[handle] = items
		inv.order = append(inv.order, handle)
	}
	for _, seen := range items[slot] {
		if seen == item {
			return
		}
	}
	items[slot] = append(items[slot], item)
}

// readItemBody returns the asset ID of the item created at b[off:].
func readItemBody(b []byte, off int, item uint64) (uint64, bool) {
	if off+itemBodySize > len(b) {
		return 0, false
	}
	body := b[off : off+itemBodySize]
	for _, m := range itemBodyMarkers {
		if !bytes.Equal(body[m.offset:m.offset+len(m.pattern)], m.pattern) {
			return 0, false
		}
	}
	if readHandle(body[1:9]) != item ||
		!bytes.Equal(body[20:28], body[33:41]) ||
		!bytes.Equal(body[45:53], body[58:66]) {
		return 0, false
	}
	return readHandle(body[71:79]), true
}

// playerObjectBefore returns the player object handle written shortly before
// the username packet starting at offset, or 0 if it cannot be found.
func (r *Reader) playerObjectBefore(offset int) uint64 {
	if r.inventory == nil || offset > len(r.b) || offset < 0 {
		return 0
	}
	from := offset - playerObjectRange
	if from < 0 {
		from = 0
	}
	i := bytes.LastIndex(r.b[from:offset], playerObjectField)
	if i < 0 {
		return 0
	}
	i += from
	if i < handleSize+1 || r.b[i-handleSize-1] != 0x1B {
		return 0
	}
	return readHandle(r.b[i-handleSize : i])
}

func (r *Reader) trackPlayerObject(handle uint64, p Player) {
	if r.inventory == nil || handle == 0 {
		return
	}
	r.inventory.players[handle] = inventoryPlayer{
		dissectID: append([]byte(nil), p.DissectID...),
		username:  p.Username,
	}
}

// playerIndexByObject returns the index of the player owning the player object, or -1.
func (r *Reader) playerIndexByObject(handle uint64) int {
	if r.inventory == nil {
		return -1
	}
	p, ok := r.inventory.players[handle]
	if !ok {
		return -1
	}
	if len(p.dissectID) == 4 {
		for i, q := range r.Header.Players {
			if bytes.Equal(q.DissectID, p.dissectID) {
				return i
			}
		}
	}
	for i, q := range r.Header.Players {
		if q.Username == p.username {
			return i
		}
	}
	return -1
}

// weaponSlot returns the slot in which the player carried the asset during the round,
// or "" when the asset is unknown for that player.
func (r *Reader) weaponSlot(player int, asset uint64) WeaponSlot {
	inv := r.inventory
	if inv == nil || asset == 0 || player < 0 {
		return ""
	}
	var found [inventorySlots]bool
	for _, handle := range inv.order {
		if r.playerIndexByObject(inv.owners[handle]) != player {
			continue
		}
		for slot, items := range inv.items[handle] {
			for _, item := range items {
				if a, ok := inv.assets[item]; ok && a == asset {
					found[slot] = true
				}
			}
		}
	}
	for _, s := range slotOrder {
		if found[s.index] {
			return s.slot
		}
	}
	return ""
}

// classifyKillWeapons sets each Kill's WeaponSlot from the inventory of the player the kill
// packet names (the finisher), not a later username correction, and WeaponItemID from that
// player's operator loadout block. It runs after all listeners.
func (r *Reader) classifyKillWeapons() {
	for i := range r.MatchFeedback {
		u := &r.MatchFeedback[i]
		if u.Type != Kill || u.WeaponID == 0 {
			continue
		}
		killer := u.feedKiller
		if killer == "" {
			killer = u.Username
		}
		idx := -1
		for j, p := range r.Header.Players {
			if p.Username == killer {
				idx = j
				break
			}
		}
		u.WeaponSlot = r.weaponSlot(idx, u.WeaponID)
		if u.WeaponSlot != "" {
			u.WeaponItemID = r.itemID(r.Header.Players[idx].Operator, u.WeaponSlot)
		}
	}
}
