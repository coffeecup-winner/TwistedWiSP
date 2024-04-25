extends GraphEdit

func _ready():
	var config = ConfigFile.new()
	config.load("res://wisp.ini")
	var wisp_exe_path = config.get_value("wisp", "executable_path")
	connect("connection_request", _on_connection_request)
	connect("disconnection_request", _on_disconnection_request)
	TwistedWisp.init(wisp_exe_path)

func _on_connection_request(from_node, from_port, to_node, to_port):
	connect_node(from_node, from_port, to_node, to_port)
	# TODO: Temp code
	TwistedWisp.enable_dsp()

func _on_disconnection_request(from_node, from_port, to_node, to_port):
	disconnect_node(from_node, from_port, to_node, to_port)
	# TODO: Temp code
	TwistedWisp.disable_dsp()

func _process(_delta):
	pass
