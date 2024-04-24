extends GraphEdit

func _ready():
	connect("connection_request", _on_connection_request)
	connect("disconnection_request", _on_disconnection_request)

func _on_connection_request(from_node, from_port, to_node, to_port):
	connect_node(from_node, from_port, to_node, to_port)

func _on_disconnection_request(from_node, from_port, to_node, to_port):
	disconnect_node(from_node, from_port, to_node, to_port)

func _process(delta):
	pass
