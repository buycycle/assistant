
    document.getElementById('send').addEventListener('click', function() {
        sendMessage();
    });
    function sendMessage() {
        var userIdInput = document.getElementById('user_id');
        var messageInput = document.getElementById('message');
        var messagesList = document.getElementById('messages');
        var userMessage = document.createElement('li');
        var currentTime = new Date();
        var formattedUserText = messageInput.value.replace(/\n/g, '<br>');
        var userId = userIdInput.value || 'User';
        userMessage.innerHTML = '<strong>' + userId + ':</strong> ' + formattedUserText + '<br><small>Sent on: ' + currentTime.toLocaleString() + '</small>';
        messagesList.appendChild(userMessage);
        // Add loading dots after the user message
        var loadingDots = document.createElement('div');
        loadingDots.className = 'dot-flashing-container';
        loadingDots.id = 'loading-dots';
        for (var i = 0; i < 3; i++) {
            var dot = document.createElement('div');
            dot.className = 'dot-flashing';
            loadingDots.appendChild(dot);
        }
        messagesList.appendChild(loadingDots); // Append the dots to the messages list
        document.getElementById('chat-window').scrollTop = document.getElementById('chat-window').scrollHeight;
    }
    document.getElementById('message').addEventListener('keydown', function(event) {
        if (event.ctrlKey && event.key === 'Enter') {
            document.getElementById('send').click();
        }
    });
    // Clear the message field after the htmx request is successfully processed
    document.body.addEventListener('htmx:afterOnLoad', function(event) {
        document.getElementById('message').value = '';
        document.getElementById('loading').style.display = 'none'; // Hide loading indicator
    });
    document.body.addEventListener('htmx:afterRequest', function(event) {
        // Remove loading dots after the response is received
        var loadingDots = document.getElementById('loading-dots');
        if (loadingDots) {
            loadingDots.remove();
        }
        var xhr = event.detail.xhr;
        var response = JSON.parse(xhr.responseText);
        var chatWindow = document.getElementById('chat-window');
        var messagesList = document.getElementById('messages');
        var message = response.messages[0];
        var date = new Date(message.created_at * 1000);
        var assistantResponse = document.createElement('li');
        assistantResponse.innerHTML = '<strong>' + message.role.charAt(0).toUpperCase() + message.role.slice(1) + ':</strong> ' + message.text + '<br><small>Sent on: ' + date.toLocaleString() + '</small>';
        messagesList.appendChild(assistantResponse);
        chatWindow.scrollTop = chatWindow.scrollHeight;
    });
    // Clear the message field after the htmx request is successfully processed
    document.body.addEventListener('htmx:afterOnLoad', function(event) {
        // No need to clear the message input here, as it's already cleared after sending the message
    });
