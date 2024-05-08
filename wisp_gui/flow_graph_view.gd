extends GraphEdit

@export
var wisp_flow_name = ""
var wisp_file_path = ""

const GROUP_WATCHES = "watches"

const NODE_NAME_CONTROL = "control"
const NODE_NAME_WATCH = "watch"

var FlowGraphNode = preload("res://flow_graph_node.tscn")
var FlowGraphNode_HSlider = preload("res://flow_graph_node_h_slider.tscn")
var FlowGraphNodeWatch = preload("res://flow_graph_node_watch.tscn")
var FlowGraphNodeWatch_Graph = preload("res://flow_graph_node_watch_graph.tscn")

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


func _on_delete_nodes_request(node_names):
	for node_name in node_names:
		# TODO: Check group instead
		var node = get_node(NodePath(node_name))
		if node is GraphNode:
			TwistedWisp.flow_remove_node(wisp_flow_name, node.wisp_node_idx)
			# TODO: Have the extension return the connection list?
			var connections_to_delete = []
			for conn in get_connection_list():
				if conn.from_node == node_name or conn.to_node == node_name:
					connections_to_delete.append(conn)
			for conn in connections_to_delete:
				disconnect_node(conn.from_node, conn.from_port, conn.to_node, conn.to_port)
		remove_child(node)
		node.queue_free()


func _on_chkbtn_dsp_toggled(toggled_on):
	if toggled_on:
		TwistedWisp.dsp_start()
	else:
		TwistedWisp.dsp_stop()


func _on_open_file_selected(f):
	for node in get_children():
		if node is GraphNode:
			# TODO: Fix debugger errors resulting from this
			remove_child(node)
	wisp_file_path = f
	wisp_flow_name = TwistedWisp.function_open(wisp_file_path)
	var node_map = {}
	for idx in TwistedWisp.flow_list_nodes(wisp_flow_name):
		var node = add_flow_node(TwistedWisp.flow_get_node_name(wisp_flow_name, idx), idx, null)
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
	elif ((event.is_action("ui_flow_graph_view_save_as") or event.is_action("ui_flow_graph_view_save"))
			and event.is_pressed()
			and not event.is_echo()):
		accept_event()
		if wisp_file_path and not event.is_action("ui_flow_graph_view_save_as"):
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


func create_node(func_name):
	match func_name:
		NODE_NAME_CONTROL: return FlowGraphNode_HSlider.instantiate()
		NODE_NAME_WATCH: return FlowGraphNodeWatch_Graph.instantiate()
		_: return FlowGraphNode.instantiate()


func add_flow_node(func_name, idx, pos):
	var node = create_node(func_name)
	var display_name = func_name
	if idx == null:
		var result = TwistedWisp.flow_add_node(wisp_flow_name, func_name)
		idx = result.idx
		func_name = result.name
		display_name = result.display_name
		node.position_offset = pos
		if func_name == NODE_NAME_CONTROL:
			node.size = Vector2(120, 60)
		elif func_name != NODE_NAME_WATCH:
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
		display_name = TwistedWisp.flow_get_node_display_name(wisp_flow_name, idx)
	
	node.title = display_name

	var metadata = TwistedWisp.function_get_metadata(func_name)
	var rows_count = max(metadata.num_inlets, metadata.num_outlets)
	
	while (node.get_child_count() < rows_count):
		node.add_child(Label.new())
	
	for i in range(0, metadata.num_inlets):
		node.set_slot_enabled_left(i, true)
	
	for i in range(0, metadata.num_outlets):
		node.set_slot_enabled_right(i, true) 
	
	if func_name == NODE_NAME_CONTROL:
		node.connect("value_changed", _on_control_value_changed)
	elif func_name == NODE_NAME_WATCH:
		node.add_to_group(GROUP_WATCHES)
	
	node.wisp_node_idx = idx
	node.wisp_func_name = func_name
	add_child(node)
	return node


func _on_control_value_changed(idx, value):
	TwistedWisp.flow_node_on_value_changed(wisp_flow_name, idx, value)


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


func _process(_delta):
	var updates = TwistedWisp.flow_get_watch_updates(wisp_flow_name)
	for node in get_children():
		if node.is_in_group(GROUP_WATCHES) and node.wisp_node_idx in updates:
			node.process_watch_updates(updates[node.wisp_node_idx])
