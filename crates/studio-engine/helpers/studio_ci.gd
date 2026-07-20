extends SceneTree

func _init() -> void:
	var failed := 0
	var checked := 0
	for path in _all_scripts("res://"):
		checked += 1
		var src := FileAccess.get_file_as_string(path)
		var script := GDScript.new()
		script.source_code = src
		var err := script.reload()
		if err != OK:
			failed += 1
			print("STUDIO_CI_FAIL: %s: script failed to compile (error %d)" % [path, err])
	print("STUDIO_CI_DONE checked=%d failed=%d" % [checked, failed])
	quit(1 if failed > 0 else 0)

func _all_scripts(root: String) -> Array:
	var out := []
	var dir := DirAccess.open(root)
	if dir == null:
		return out
	dir.list_dir_begin()
	var name := dir.get_next()
	while name != "":
		if name.begins_with("."):
			name = dir.get_next()
			continue
		var full := root.path_join(name)
		if dir.current_is_dir():
			out.append_array(_all_scripts(full))
		elif name.ends_with(".gd"):
			out.append(full)
		name = dir.get_next()
	dir.list_dir_end()
	return out
