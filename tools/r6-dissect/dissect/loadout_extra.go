package dissect

// Operators the generated tables do not know (R6Koc patch 7).
//
// gameOperatorNames (game_operators.go) is generated from the wnc-replay name table, which has no
// entry for Noor. It lists 0x64D62DB918 as "DIAMOND" without a matching operator: that block is
// Solid Snake's. Both findings come from the local Y11S3 replays: in every round where a player
// has operator ID 444310693746 (Solid Snake) or 456757346397 (Noor) in the header, exactly one
// loadout block remains unmatched after pairing the other nine operators by name, and it is always
// the same block (0x64D62DB918 for Snake, 0x679C49B955 for Noor).
//
// The operator name tables in this package (operator_string.go, operator_roles.go) predate both
// operators and are generated, so the aliases live here instead of in those files.

// extraGameOperatorNames adds loadout block operator IDs missing from gameOperatorNames.
var extraGameOperatorNames = map[uint64]string{
	0x679C49B955: "NOOR",
}

// operatorLoadoutNames maps dissect operator IDs that have no name in operator_string.go to the
// name their loadout block has in gameOperatorNames / extraGameOperatorNames.
var operatorLoadoutNames = map[Operator]string{
	444310693746: "DIAMOND", // Solid Snake
	456757346397: "NOOR",
}

// gameOperatorName returns the operator name of a loadout block operator ID.
func gameOperatorName(id uint64) (string, bool) {
	if n, ok := gameOperatorNames[id]; ok {
		return n, true
	}
	n, ok := extraGameOperatorNames[id]
	return n, ok
}

// loadoutOperatorName is the name used to pair a player's operator with a loadout block.
func loadoutOperatorName(op Operator) string {
	if n, ok := operatorLoadoutNames[op]; ok {
		return n
	}
	return op.String()
}
