extends GraphEdit

const GROUP_NODES = "nodes"

# Known WiSP function names
const NODE_NAME_CONTROL = "control"
const NODE_NAME_BUTTON = "button"
const NODE_NAME_TOGGLE = "toggle"
const NODE_NAME_WATCH = "watch"
const NODE_NAME_GRAPH = "graph"
const NODE_NAME_BUFFER = "buffer"

const FlowGraphNode_Generic = preload("res://flow_graph_node.tscn")
const FlowGraphNode_HSlider = preload("res://flow_graph_node_h_slider.tscn")
const FlowGraphNode_Button = preload("res://flow_graph_node_button.tscn")
const FlowGraphNode_Toggle = preload("res://flow_graph_node_input_toggle.tscn")
const FlowGraphNode_Watch = preload("res://flow_graph_node_watch.tscn")
const FlowGraphNode_Watch_Graph = preload("res://flow_graph_node_watch_graph.tscn")
const FlowGraphNode_Buffer = preload("res://flow_graph_node_buffer.tscn")

const FlowGraphNodeSelector = preload("res://flow_graph_node_selector.tscn")

var wisp: TwistedWisp
var flow: TwistedWispFlow
var flow_file_path = ""
var selected_nodes = []


func _ready():
	flow = wisp.create_flow()


func _is_node_hover_valid(from_node: StringName, _from_port: int, to_node: StringName, _to_port: int) -> bool:
	if from_node != to_node:
		return true
	var node = get_node(NodePath(from_node))
	var metadata = wisp.get_function_metadata(node.flow_node.function_name())
	return metadata.is_lag


func _on_connection_request(from_node, from_port, to_node, to_port):
	connect_node(from_node, from_port, to_node, to_port)
	flow.connect_nodes(
		get_node(NodePath(from_node)).flow_node,
		from_port,
		get_node(NodePath(to_node)).flow_node,
		to_port)


func _on_disconnection_request(from_node, from_port, to_node, to_port):
	disconnect_node(from_node, from_port, to_node, to_port)
	flow.disconnect_nodes(
		get_node(NodePath(from_node)).flow_node,
		from_port,
		get_node(NodePath(to_node)).flow_node,
		to_port)


func _on_node_selected(node):
	selected_nodes.append(node)


func _on_node_deselected(node):
	selected_nodes.erase(node)


func _on_delete_nodes_request(node_names):
	for node_name in node_names:
		var node = get_node(NodePath(node_name))
		if node.is_in_group(GROUP_NODES):
			flow.remove_node(node.flow_node)
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
		wisp.start_dsp()
	else:
		wisp.stop_dsp()


func _on_open_file_selected(f):
	clear_connections()
	for node in get_children():
		if node.is_in_group(GROUP_NODES):
			remove_child(node)
			node.queue_free()
	flow_file_path = f
	flow = wisp.load_flow_from_file(flow_file_path)
	var node_map = {}
	for flow_node in flow.list_nodes():
		var node = add_flow_node(flow_node, false, null)
		node_map[flow_node.id()] = node
	for conn in flow.list_connections():
		connect_node(
			node_map[conn.from.id()].name,
			conn.output_index,
			node_map[conn.to.id()].name,
			conn.input_index)


func _on_save_file_selected(f):
	flow_file_path = f
	flow.save_to_file(flow_file_path)


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
		if flow_file_path and not event.is_action("ui_flow_graph_view_save_as"):
			flow.save_to_file(flow_file_path)
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
		if event.pressed:
			if event.keycode == KEY_N:
				accept_event()
				var selector = FlowGraphNodeSelector.instantiate()
				selector.set_position(get_local_mouse_position())
				add_child(selector)
				selector.grab_focus()
			elif event.keycode == KEY_L:
				accept_event()
				if len(selected_nodes) == 1:
					var node = selected_nodes[0]
					if node is FlowGraphNode:
						# TODO: Change this when there are multiple controls
						if node.flow_node.function_name() == NODE_NAME_CONTROL:
							node.flow_node.learn_midi_cc()


func create_node(func_name):
	match func_name:
		NODE_NAME_CONTROL: return FlowGraphNode_HSlider.instantiate()
		NODE_NAME_BUTTON: return FlowGraphNode_Button.instantiate()
		NODE_NAME_TOGGLE: return FlowGraphNode_Toggle.instantiate()
		NODE_NAME_WATCH: return FlowGraphNode_Watch.instantiate()
		NODE_NAME_GRAPH: return FlowGraphNode_Watch_Graph.instantiate()
		NODE_NAME_BUFFER: return FlowGraphNode_Buffer.instantiate()
		_: return FlowGraphNode_Generic.instantiate()


func data_type_to_slot_type(data_type):
	match data_type:
		"float": return 0
		"array": return 1
		_: return -1


func slot_type_to_color(slot_type) -> Color:
	match slot_type:
		0: return Color.WHITE
		1: return Color.GRAY
		_: return Color.RED


func add_flow_node(flow_node: TwistedWispFlowNode, is_new: bool, local_pos):
	var node: FlowGraphNode
	var func_name = flow_node.function_name()
	var display_name = func_name
	if is_new:
		node = create_node(func_name)
		node.position_offset = (self.scroll_offset + local_pos) / self.zoom
		if snapping_enabled:
			node.position_offset = (node.position_offset / self.snapping_distance).floor() * self.snapping_distance
		if func_name == NODE_NAME_CONTROL:
			node.size = Vector2(120, 60)
		elif func_name != NODE_NAME_GRAPH:
			node.size = Vector2(80, 80)
		flow_node.set_property_value("x", int(node.position_offset.x))
		flow_node.set_property_value("y", int(node.position_offset.y))
		flow_node.set_property_value("w", int(node.size.x))
		flow_node.set_property_value("h", int(node.size.y))
	else:
		node = create_node(func_name)
		node.position_offset.x = flow_node.get_property_value("x")
		node.position_offset.y = flow_node.get_property_value("y")
		node.size.x = flow_node.get_property_value("w")
		node.size.y = flow_node.get_property_value("h")
		display_name = flow_node.display_name()
	
	node.title = display_name
	
	var metadata = wisp.get_function_metadata(func_name)
	var rows_count = max(len(metadata.inlets), len(metadata.outlets))
	
	while (node.get_child_count() < rows_count):
		node.add_child(Label.new())
	
	for i in range(0, len(metadata.inlets)):
		var inlet = metadata.inlets[i]
		node.set_slot_enabled_left(i, true)
		var slot_type = data_type_to_slot_type(inlet)
		node.set_slot_type_left(i, slot_type)
		node.set_slot_color_left(i, slot_type_to_color(slot_type))
	
	for i in range(0, len(metadata.outlets)):
		var outlet = metadata.outlets[i]
		node.set_slot_enabled_right(i, true) 
		var slot_type = data_type_to_slot_type(outlet)
		node.set_slot_type_right(i, slot_type)
		node.set_slot_color_right(i, slot_type_to_color(slot_type))
	
	node.add_to_group(GROUP_NODES)
	
	node.flow_node = flow_node
	add_child(node)
	return node


func _on_end_node_move():
	for node in get_children():
		if node.is_in_group(GROUP_NODES) and node.selected:
			node.flow_node.set_property_value("x", int(node.position_offset.x))
			node.flow_node.set_property_value("y", int(node.position_offset.y))
			node.flow_node.set_property_value("w", int(node.size.x))
			node.flow_node.set_property_value("h", int(node.size.y))


func _process(_delta):
	flow.fetch_watch_updates()
