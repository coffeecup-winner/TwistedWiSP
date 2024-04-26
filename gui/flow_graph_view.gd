extends GraphEdit

@export
var wisp_flow_name = ""

func _ready():
	var config = ConfigFile.new()
	config.load("res://wisp.ini")
	var wisp_exe_path = config.get_value("wisp", "executable_path")
	connect("connection_request", _on_connection_request)
	connect("disconnection_request", _on_disconnection_request)
	TwistedWisp.init(wisp_exe_path)

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
		TwistedWisp.enable_dsp()
	else:
		TwistedWisp.disable_dsp()
