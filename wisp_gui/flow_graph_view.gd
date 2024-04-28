extends GraphEdit

@export
var wisp_flow_name = ""
var wisp_file_path = ""

var FlowGraphNode = preload("res://flow_graph_node.tscn")
var FlowGraphNodeSelector = preload("res://flow_graph_node_selector.tscn")

func _ready():
	connect("connection_request", _on_connection_request)
	connect("disconnection_request", _on_disconnection_request)


func _on_connection_request(from_node, from_port, to_node, to_port):
	connect_node(from_node, from_port, to_node, to_port)
	TwistedWisp.flow_connect(
		wisp_flow_name,
		get_node(NodePath(from_node)).wisp_node_idx,
		from_port,
		get_node(NodePath(to_node)).wisp_node_idx,
		to_port)


func _on_disconnection_request(from_node, from_port, to_node, to_port):
	disconnect_node(from_node, from_port, to_node, to_port)
	TwistedWisp.flow_disconnect(
		wisp_flow_name,
		get_node(NodePath(from_node)).wisp_node_idx,
		from_port,
		get_node(NodePath(to_node)).wisp_node_idx,
		to_port)


func _on_chkbtn_dsp_toggled(toggled_on):
	if toggled_on:
		TwistedWisp.dsp_start()
	else:
		TwistedWisp.dsp_stop()


func _on_open_file_selected(f):
	wisp_file_path = f
	wisp_flow_name = TwistedWisp.function_open(wisp_file_path)
	var node_map = {}
	for idx in TwistedWisp.flow_list_nodes(wisp_flow_name):
		var node = add_flow_node(TwistedWisp.flow_get_node_name(wisp_flow_name, idx), idx)
		node_map[idx] = node
	for idx in TwistedWisp.flow_list_connections(wisp_flow_name):
		var conn = TwistedWisp.flow_get_connection(wisp_flow_name, idx)
		connect_node(
			node_map[conn.from].name,
			conn.output_index,
			node_map[conn.to].name,
			conn.input_index)


func _on_save_file_selected(f):
	wisp_file_path = f
	TwistedWisp.function_save(wisp_flow_name, wisp_file_path)


func _on_gui_input(event):
	if event.is_action("ui_flow_graph_view_open") and event.is_pressed() and not event.is_echo():
		accept_event()
		var fd = FileDialog.new()
		fd.access = FileDialog.ACCESS_FILESYSTEM
		fd.dialog_hide_on_ok = true
		fd.file_mode = FileDialog.FILE_MODE_OPEN_FILE
		fd.filters = PackedStringArray(["*.twf ; TwistedWiSP Flow Files"])
		fd.title = "Open a flow graph"
		fd.use_native_dialog = true
		fd.connect("file_selected", _on_open_file_selected)
		fd.popup()
	elif event.is_action("ui_flow_graph_view_save") and event.is_pressed() and not event.is_echo():
		accept_event()
		if wisp_file_path:
			TwistedWisp.function_save(wisp_flow_name, wisp_file_path)
		else:
			var fd = FileDialog.new()
			fd.access = FileDialog.ACCESS_FILESYSTEM
			fd.dialog_hide_on_ok = true
			fd.file_mode = FileDialog.FILE_MODE_SAVE_FILE
			fd.filters = PackedStringArray(["*.twf ; TwistedWiSP Flow Files"])
			fd.title = "Save a flow graph"
			fd.use_native_dialog = true
			fd.connect("file_selected", _on_save_file_selected)
			fd.popup()
	elif event is InputEventKey:
		if event.pressed and event.keycode == KEY_N:
			accept_event()
			var selector = FlowGraphNodeSelector.instantiate()
			selector.set_position(get_local_mouse_position())
			add_child(selector)
			selector.grab_focus()


func add_flow_node(name, idx):
	var node = FlowGraphNode.instantiate()
	node.title = name
	
	var metadata = TwistedWisp.function_get_metadata(name)
	var rows_count = max(metadata.num_inlets, metadata.num_outlets)
	
	for i in range(0, rows_count):
		node.add_child(Label.new())
	
	for i in range(0, metadata.num_inlets):
		node.set_slot_enabled_left(i, true)
	
	for i in range(0, metadata.num_outlets):
		node.set_slot_enabled_right(i, true)

	if idx == null:
		idx = TwistedWisp.flow_add_node(wisp_flow_name, name)
		node.position_offset = self.position
		node.size = Vector2(80, 80)
		TwistedWisp.flow_set_node_coordinates(
			wisp_flow_name,
			node.wisp_node_idx,
			int(node.position_offset.x),
			int(node.position_offset.y),
			int(node.size.x),
			int(node.size.y))
	else:
		var coords = TwistedWisp.flow_get_node_coordinates(wisp_flow_name, idx)
		node.position_offset.x = coords.x
		node.position_offset.y = coords.y
		node.size.x = coords.w
		node.size.y = coords.h
	
	node.wisp_node_idx = idx
	add_child(node)
	return node


func _on_end_node_move():
	for node in get_children():
		if node is GraphNode and node.selected:
			TwistedWisp.flow_set_node_coordinates(
				wisp_flow_name,
				node.wisp_node_idx,
				int(node.position_offset.x),
				int(node.position_offset.y),
				int(node.size.x),
				int(node.size.y))
