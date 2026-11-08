extends SceneTree

var failures := 0
var checked := 0

func _init() -> void:
	var args := OS.get_cmdline_user_args()
	var dir := "res://merged" if args.is_empty() else args[0]
	_check_dir(dir)
	print("checked %d file(s), %d failure(s)" % [checked, failures])
	quit(1 if failures > 0 or checked == 0 else 0)

func _check_dir(path: String) -> void:
	var d := DirAccess.open(path)
	if d == null:
		push_error("cannot open %s" % path)
		failures += 1
		return
	for sub in d.get_directories():
		_check_dir(path.path_join(sub))
	for f in d.get_files():
		if f.ends_with(".tscn") or f.ends_with(".tres"):
			_check_file(path.path_join(f))

func _count_sections(path: String) -> int:
	var text := FileAccess.get_file_as_string(path)
	var n := 0
	for line in text.split("\n"):
		if line.begins_with("[node "):
			n += 1
	return n

func _count_owned(root: Node, node: Node) -> int:
	var n := 0
	for c in node.get_children(true):
		if c.owner == root:
			n += 1
		n += _count_owned(root, c)
	return n

func _check_file(path: String) -> void:
	checked += 1
	var res := ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_IGNORE)
	if res == null:
		printerr("FAIL load: %s" % path)
		failures += 1
		return
	if res is PackedScene:
		var text := FileAccess.get_file_as_string(path)
		if text.contains("parent_id_path") or text.contains("[editable") or _first_node_is_instance(text):
			print("ok (loaded)       %s" % path)
			return
		var scene := (res as PackedScene).instantiate()
		if scene == null:
			printerr("FAIL instantiate: %s" % path)
			failures += 1
			return
		var expected := _count_sections(path)
		var got := 1 + _count_owned(scene, scene)
		scene.free()
		if got != expected:
			printerr("FAIL %s: %d node section(s) but %d node(s) after instantiate" % [path, expected, got])
			failures += 1
			return
		print("ok (instantiated) %s" % path)
	else:
		print("ok (loaded)       %s" % path)

func _first_node_is_instance(text: String) -> bool:
	for line in text.split("\n"):
		if line.begins_with("[node "):
			return line.contains(" instance=") and not line.contains(" parent=")
	return false
