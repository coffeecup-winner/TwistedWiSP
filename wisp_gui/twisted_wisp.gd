extends Control

var FlowGraphView = preload("res://flow_graph_view.tscn")

func _ready():
	var config = ConfigFile.new()
	config.load("res://wisp.ini")
	var wisp_exe_path = config.get_value("wisp", "executable_path")
	var wisp_core_path = config.get_value("wisp", "core_path")
	TwistedWisp.init(wisp_exe_path, wisp_core_path)
	var graph = FlowGraphView.instantiate()
	# TODO: Remove this
	var func_name = TwistedWisp.function_create()
	TwistedWisp.function_set_main(func_name)
	graph.wisp_flow_name = func_name
	add_child(graph)
	graph.grab_focus()


func _process(_delta):
	pass
