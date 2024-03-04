// Define the mappings
def map_branch_to_env = [
    "dev": "dev",
    "staging": "staging",
    "main": "live"
]
def map_branch_to_ab = [
    "dev": "canary",
    "staging": "canary",
    "main": "stable"
]
// Set dev as default
def image_tag = "dev-${env.BUILD_NUMBER}"
def environment = "dev"
def ab = "canary"
// Check the branch name and set variables accordingly
if (env.BRANCH_NAME == "main" || env.BRANCH_NAME == "staging") {
    image_tag = "${env.BRANCH_NAME}-${env.BUILD_NUMBER}"
    environment = "${map_branch_to_env[env.BRANCH_NAME]}"
    ab = "${map_branch_to_ab[env.BRANCH_NAME]}"
}
// Simple switch to control skipping the test execution, default is false
def skipTests = false
pipeline {
    agent any
    options {
        disableConcurrentBuilds()
    }
    stages {
        stage('Build') {
            steps {
                withCredentials([string(credentialsId: 'bot-env', variable: 'S3_ENV_PATH')]) {
                    sh "aws s3 cp ${S3_ENV_PATH} rust_bot/.env"
                }
                script {
                    if (environment == "live") {
                        app = docker.build("chat-bot", "-f docker/main.dockerfile --build-arg ENVIRONMENT=${environment} --build-arg AB=${ab} .")
                    } else {
                        app = docker.build("chat-bot", "-f docker/dev.dockerfile --build-arg ENVIRONMENT=${environment} --build-arg AB=${ab} .")
                    }
                }
            }
        }
        stage('Test') {
                expression { !skipTests } // Only run tests if skipTests is false
            }
            steps {
                script {
                    app.inside {
//                        sh 'cargo test --manifest-path rust_bot/Cargo.toml'
                    }
                }
            }
        }
        stage('Push Docker image') {
            steps {
                withCredentials([string(credentialsId: 'recommendation-ecr-url', variable: 'ECR_URL')]) {
                    script {
                        docker.withRegistry("${ECR_URL}", 'ecr:eu-central-1:aws-credentials-ecr') {
                            app.push(image_tag)
                            app.push("latest")
                        }
                    }
                }
            }
}
        stage("Modify HELM chart") {
            steps {
                sh "make push IMAGE_TAG=${image_tag} ENV=${environment}"
            }
        }
        stage("Sync Chart") {
            steps {
                withCredentials([string(credentialsId: 'argocd-token', variable: 'TOKEN')]) {
                    script {
                        env.namespace = environment
                    }
                    sh '''
                      set +x
                      argocd app sync chat-bot-$namespace --server argocd.cube-gebraucht.com --auth-token $TOKEN
                      argocd app wait chat-bot-$namespace --server argocd.cube-gebraucht.com --auth-token $TOKEN
                    '''
                }
            }
        }
    }
}

