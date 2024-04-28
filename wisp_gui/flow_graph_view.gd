extends GraphEdit

@export
var wisp_flow_name = ""

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


func _on_gui_input(event):
	if event is InputEventKey:
		if event.pressed and event.keycode == KEY_N:
			accept_event()
			var selector = FlowGraphNodeSelector.instantiate()
			selector.set_position(get_local_mouse_position())
			add_child(selector)
			selector.grab_focus()


func add_flow_node(node):
	var idx = TwistedWisp.flow_add_node(wisp_flow_name, node.title)
	node.wisp_node_idx = idx
	add_child(node)
